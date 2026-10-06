use std::sync::Arc;

use super::errors::ProviderError;
use super::fake::*;
use super::registry::{Binding, ProviderSessions};
use super::routing::*;
use super::sink::*;
use crate::plugin_rpc::PluginRpcError;
use planeai_plugin_contract::provider::{
    self as protocol, error_code, ProviderErrorCode, ProviderSessionStatus, StopReason,
    EVENT_NOTIFICATION, STATUS_NOTIFICATION,
};
use serde_json::Value;

fn target(session_id: &str) -> StopTarget {
    StopTarget::of(session_id, "chat:claude").unwrap()
}

fn launched_ids(sessions: &ProviderSessions) -> Vec<String> {
    let mut ids: Vec<String> = sessions.launched().into_iter().map(|(id, _)| id).collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_start_binds_before_it_asks_and_undoes_itself_when_refused() {
    let runtime = FakeRuntime::running(&[]);
    start(&runtime, &context("s1"), None).await.unwrap();
    // Notifications the sidecar sends while starting must find the session bound.
    assert_eq!(
        *runtime.requests.lock().unwrap(),
        [(protocol::START.to_string(), true)]
    );
    assert_eq!(launched_ids(&runtime.sessions), Vec::<String>::new());

    runtime.failing.lock().unwrap().push(protocol::START);
    assert!(start(&runtime, &context("s2"), None).await.is_err());
    assert!(runtime.sessions.binding("s2").is_none());
    assert!(!runtime
        .sessions
        .sessions
        .lock()
        .unwrap()
        .get("s2")
        .is_some_and(|session| session.launching));
    // A start cut short may still run in the sidecar, so it is told to stop.
    assert_eq!(
        runtime.sent().last().map(String::as_str),
        Some(protocol::STOP)
    );
}

#[tokio::test]
async fn sessions_resume_once_per_sidecar_instance() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    ensure(&runtime, "s1").await.unwrap();
    ensure(&runtime, "s1").await.unwrap();
    assert_eq!(runtime.sent(), [protocol::RESUME]);
    *runtime.instance.lock().unwrap() = Some(2);
    ensure(&runtime, "s1").await.unwrap();
    assert_eq!(runtime.sent(), [protocol::RESUME, protocol::RESUME]);
}

#[tokio::test]
async fn a_refused_resume_leaves_the_session_unbound() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    runtime.failing.lock().unwrap().push(protocol::RESUME);
    assert!(ensure(&runtime, "s1").await.is_err());
    assert!(runtime.sessions.binding("s1").is_none());
}

#[tokio::test]
async fn ended_sessions_are_never_resumed() {
    let runtime = FakeRuntime::running(&[("s1", "archived")]);
    assert!(ensure(&runtime, "s1")
        .await
        .unwrap_err()
        .message
        .contains("not active"));
    assert!(runtime.sent().is_empty());
}

#[tokio::test]
async fn stop_reaches_the_plugin_even_when_no_sidecar_drives_the_session() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    ensure(&runtime, "s1").await.unwrap();
    *runtime.instance.lock().unwrap() = Some(2);
    stop(&runtime, &target("s1"), StopReason::Archive).await;
    assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
    assert!(runtime.sessions.binding("s1").is_none());

    // Never bound in this run.
    stop(&runtime, &target("s2"), StopReason::Destroy).await;
    assert_eq!(
        runtime.params.lock().unwrap()[protocol::STOP]["session_id"],
        "s2"
    );
}

#[tokio::test]
async fn a_stop_forgets_everything_about_its_session_at_once() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    handoff(&runtime, "s1").await.unwrap();
    runtime.sessions.begin_launch("s1");
    stop(&runtime, &target("s1"), StopReason::Archive).await;
    assert!(runtime.sessions.sessions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_stopped_plugin_hears_of_ended_sessions_through_reconciliation() {
    let runtime = FakeRuntime::running(&[]);
    *runtime.instance.lock().unwrap() = None;
    stop(&runtime, &target("s1"), StopReason::Archive).await;
    assert!(runtime.sent().is_empty());
}

#[tokio::test]
async fn a_resume_carries_whether_the_session_is_handed_off() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    handoff(&runtime, "s1").await.unwrap();
    assert_eq!(
        runtime.params.lock().unwrap()[protocol::RESUME]["handed_off"],
        false
    );
    // The sidecar restarts while the terminal still runs.
    *runtime.instance.lock().unwrap() = Some(2);
    ensure(&runtime, "s1").await.unwrap();
    assert_eq!(
        runtime.params.lock().unwrap()[protocol::RESUME]["handed_off"],
        true
    );
}

#[tokio::test]
async fn a_plugin_that_lost_a_session_gets_it_resumed_once() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    *runtime.loses_session_on.lock().unwrap() = Some(protocol::SEND);
    send(&runtime, "s1", "hi").await.unwrap();
    assert_eq!(
        runtime.sent(),
        [
            protocol::RESUME,
            protocol::SEND,
            protocol::RESUME,
            protocol::SEND
        ]
    );
}

#[tokio::test]
async fn failures_say_why_to_the_session_ui() {
    let runtime = FakeRuntime::running(&[("s1", "active"), ("done", "archived")]);
    let code = |error: ProviderError| error.code;
    assert_eq!(
        code(
            send(&runtime, "s1", &"x".repeat(protocol::MAX_PROMPT_BYTES))
                .await
                .unwrap_err()
        ),
        ProviderErrorCode::PromptTooLarge
    );
    assert_eq!(
        code(send(&runtime, "done", "hi").await.unwrap_err()),
        ProviderErrorCode::NotRunning
    );
    handoff(&runtime, "s1").await.unwrap();
    assert_eq!(
        code(send(&runtime, "s1", "hi").await.unwrap_err()),
        ProviderErrorCode::HandedOff
    );
    let from_plugin = |code: i64| {
        ProviderError::from(PluginRpcError::Rpc {
            code,
            message: "refused".into(),
        })
        .code
    };
    assert_eq!(
        from_plugin(error_code::UNAVAILABLE),
        ProviderErrorCode::Unavailable
    );
    assert_eq!(
        from_plugin(error_code::HANDED_OFF),
        ProviderErrorCode::HandedOff
    );
    assert_eq!(from_plugin(-32000), ProviderErrorCode::PluginError);
    assert_eq!(
        ProviderError::from(PluginRpcError::NotSent("queue timed out".into())).code,
        ProviderErrorCode::PluginError
    );
    assert_eq!(
        serde_json::to_value(ProviderError::new(ProviderErrorCode::NotRunning, "gone")).unwrap(),
        serde_json::json!({ "code": "not_running", "message": "gone" })
    );
}

#[test]
fn event_seq_only_increases_per_session() {
    let sessions = Arc::new(ProviderSessions::default());
    bound(&sessions, "s1", 1);
    let status = Arc::new(FakeStatus::default());
    let sink = NotificationSink::new(status.clone(), "chat", 1, sessions);
    for seq in [1, 3, 3, 2, 0] {
        sink.route(EVENT_NOTIFICATION, event_params("s1", seq));
    }
    assert_eq!(status.take(), ["event s1 1", "event s1 3"]);
}

#[tokio::test]
async fn a_session_archived_while_resuming_is_stopped_after_the_resume() {
    let runtime = Arc::new(FakeRuntime::running(&[("s1", "active")]));
    let gate = Arc::new(tokio::sync::Notify::new());
    *runtime.resume_gate.lock().unwrap() = Some(gate.clone());
    let resuming = tokio::spawn({
        let runtime = runtime.clone();
        async move { ensure(&*runtime, "s1").await.map(|_| ()) }
    });
    while runtime.sent().is_empty() {
        tokio::task::yield_now().await;
    }
    runtime
        .statuses
        .lock()
        .unwrap()
        .insert("s1".into(), "archived".into());
    let stopping = tokio::spawn({
        let runtime = runtime.clone();
        async move { stop(&*runtime, &target("s1"), StopReason::Archive).await }
    });
    tokio::task::yield_now().await;
    // The stop waits for the resume it would otherwise race.
    assert_eq!(runtime.sent(), [protocol::RESUME]);
    gate.notify_one();
    resuming.await.unwrap().unwrap();
    stopping.await.unwrap();
    assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
    assert!(runtime.sessions.binding("s1").is_none());
    // Anything that asks for it later finds it archived.
    *runtime.resume_gate.lock().unwrap() = None;
    assert!(ensure(&*runtime, "s1").await.is_err());
    assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
}

#[tokio::test]
async fn reconciliation_stops_ended_sessions_but_not_launching_ones() {
    let runtime = FakeRuntime::running(&[("live", "active"), ("archived", "active")]);
    for id in ["live", "archived"] {
        ensure(&runtime, id).await.unwrap();
    }
    start(&runtime, &context("launching"), None).await.unwrap();
    runtime
        .statuses
        .lock()
        .unwrap()
        .insert("archived".into(), "archived".into());
    runtime.requests.lock().unwrap().clear();

    assert_eq!(reconcile(&runtime).await, ["archived"]);
    assert_eq!(runtime.sent(), [protocol::STOP]);
    assert!(runtime.sessions.binding("archived").is_none());
    assert!(runtime.sessions.binding("live").is_some());
    assert!(runtime.sessions.binding("launching").is_some());
}

#[test]
fn a_sink_handles_only_provider_notifications() {
    let sink = NotificationSink::new(
        FakeStatus::default(),
        "chat",
        1,
        Arc::new(ProviderSessions::default()),
    );
    assert!(sink.handles(EVENT_NOTIFICATION));
    assert!(sink.handles(STATUS_NOTIFICATION));
    assert!(!sink.handles("host.sessions.prompt"));
}

#[test]
fn bindings_track_ownership() {
    let sessions = ProviderSessions::default();
    sessions.bind(
        "s1",
        Binding {
            plugin_id: "a".into(),
            instance: 1,
        },
    );
    assert!(sessions.is_driven_by("s1", "a", 1));
    assert!(!sessions.is_driven_by("s1", "b", 1));
    assert!(!sessions.is_driven_by("s2", "a", 1));
    // A restarted sidecar is a new instance: the previous one no longer drives the session.
    assert!(!sessions.is_driven_by("s1", "a", 2));
    assert!(sessions.unbind("s1").is_some());
    assert!(!sessions.is_driven_by("s1", "a", 1));
}

#[test]
fn reconciliation_skips_sessions_still_launching() {
    // A launching session has no row yet, which reconciliation would read as deleted.
    let sessions = ProviderSessions::default();
    let binding = Binding {
        plugin_id: "a".into(),
        instance: 1,
    };
    sessions.bind("s1", binding.clone());
    sessions.bind("s2", binding);
    sessions.begin_launch("s2");
    assert_eq!(launched_ids(&sessions), ["s1"]);
    sessions.end_launch("s2");
    assert_eq!(launched_ids(&sessions), ["s1", "s2"]);
}

#[test]
fn a_stopped_instance_releases_only_its_own_sessions() {
    let sessions = ProviderSessions::default();
    let binding = |plugin_id: &str, instance| Binding {
        plugin_id: plugin_id.into(),
        instance,
    };
    sessions.bind("s1", binding("a", 1));
    sessions.bind("s2", binding("a", 1));
    sessions.bind("s3", binding("a", 2));
    sessions.bind("s4", binding("b", 1));
    let mut released = sessions.unbind_instance("a", 1);
    released.sort();
    assert_eq!(released, ["s1", "s2"]);
    assert!(sessions.binding("s1").is_none());
    assert!(sessions.is_driven_by("s3", "a", 2));
    assert!(sessions.is_driven_by("s4", "b", 1));
}

#[test]
fn reconciliation_stops_sessions_that_ended_or_vanished() {
    assert_eq!(ended_reason(Some("active")), None);
    assert_eq!(ended_reason(Some("archived")), Some(StopReason::Archive));
    assert_eq!(ended_reason(Some("destroyed")), Some(StopReason::Destroy));
    assert_eq!(ended_reason(None), Some(StopReason::Destroy));
    assert_eq!(
        serde_json::to_value(StopReason::Archive).unwrap(),
        "archive"
    );
}

fn bound(sessions: &ProviderSessions, session_id: &str, instance: u64) {
    sessions.bind(
        session_id,
        Binding {
            plugin_id: "chat".into(),
            instance,
        },
    );
}

fn event_params(session_id: &str, seq: u64) -> Value {
    serde_json::json!({ "session_id": session_id, "seq": seq, "payload": {} })
}

fn status_params(session_id: &str, status: &str) -> Value {
    serde_json::json!({ "session_id": session_id, "status": status })
}

#[tokio::test]
async fn an_abandoned_launch_disowns_and_destroys_its_session() {
    let runtime = Arc::new(FakeRuntime::running(&[]));
    let launch = launch(runtime.clone(), &context("s1"), None).await.unwrap();
    assert_eq!(runtime.status.take(), ["own s1"]);
    drop(launch);
    // The stop runs in the background, since a launch can be dropped outside async code.
    while !runtime.sent().contains(&protocol::STOP.to_string()) {
        tokio::task::yield_now().await;
    }
    assert_eq!(runtime.status.take(), ["disown s1", "release s1"]);
    assert!(runtime.sessions.binding("s1").is_none());
}

#[tokio::test]
async fn a_committed_launch_becomes_reconcilable() {
    let runtime = Arc::new(FakeRuntime::running(&[]));
    let launch = launch(runtime.clone(), &context("s1"), None).await.unwrap();
    launch.commit();
    assert_eq!(launched_ids(&runtime.sessions), ["s1"]);
    assert_eq!(runtime.sent(), [protocol::START]);
    assert_eq!(runtime.status.take(), ["own s1"]);
}

#[tokio::test]
async fn a_refused_launch_gives_status_back() {
    let runtime = Arc::new(FakeRuntime::running(&[]));
    runtime.failing.lock().unwrap().push(protocol::START);
    assert!(launch(runtime.clone(), &context("s1"), None).await.is_err());
    assert_eq!(runtime.status.take(), ["own s1", "disown s1"]);
}

#[test]
fn notifications_from_a_previous_sidecar_instance_are_dropped() {
    let sessions = Arc::new(ProviderSessions::default());
    bound(&sessions, "s1", 1);
    let status = Arc::new(FakeStatus::default());
    let stale = NotificationSink::new(status.clone(), "chat", 2, sessions.clone());
    stale.route(STATUS_NOTIFICATION, status_params("s1", "busy"));
    assert!(status.take().is_empty());

    let current = NotificationSink::new(status.clone(), "chat", 1, sessions.clone());
    current.route(STATUS_NOTIFICATION, status_params("s1", "busy"));
    current.route(EVENT_NOTIFICATION, event_params("s1", 3));
    assert_eq!(status.take(), ["Busy s1", "event s1 3"]);
}

#[test]
fn an_exited_session_is_unbound_before_its_status_applies() {
    let sessions = Arc::new(ProviderSessions::default());
    bound(&sessions, "s1", 1);
    let status = Arc::new(FakeStatus::default());
    let sink = NotificationSink::new(status.clone(), "chat", 1, sessions.clone());
    sink.route(STATUS_NOTIFICATION, status_params("s1", "exited"));
    assert_eq!(status.take(), ["Exited s1"]);
    assert!(sessions.binding("s1").is_none());
    // Whatever it sends afterwards is no longer its own.
    sink.route(STATUS_NOTIFICATION, status_params("s1", "idle"));
    assert!(status.take().is_empty());
}

#[tokio::test]
async fn stopping_a_session_ends_a_running_turn_and_forgets_its_lock() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    ensure(&runtime, "s1").await.unwrap();
    stop(&runtime, &target("s1"), StopReason::Archive).await;
    assert_eq!(runtime.status.take(), ["release s1"]);
    assert_eq!(runtime.sent(), [protocol::RESUME, protocol::STOP]);
    assert!(runtime.sessions.locks.lock().unwrap().is_empty());
}

#[test]
fn a_stopped_sidecar_releases_the_turns_it_was_running() {
    let sessions = ProviderSessions::default();
    bound(&sessions, "s1", 1);
    bound(&sessions, "s2", 2);
    let status = FakeStatus::default();
    runtime_stopped(&status, &sessions, "chat", 1);
    assert_eq!(status.take(), ["release s1"]);
    assert!(sessions.binding("s2").is_some());
}

#[tokio::test]
async fn prompts_are_refused_while_the_session_continues_in_a_terminal() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    assert_eq!(
        handoff(&runtime, "s1").await.unwrap(),
        ["claude", "--resume", "s1"]
    );
    assert!(send(&runtime, "s1", "hi")
        .await
        .unwrap_err()
        .message
        .contains("continues in a terminal"));
    handback(&runtime, "s1").await.unwrap();
    send(&runtime, "s1", "hi").await.unwrap();
    assert_eq!(
        runtime.sent(),
        [
            protocol::RESUME,
            protocol::HANDOFF,
            protocol::HANDBACK,
            protocol::SEND
        ]
    );
}

/// Unbinds the session while a status applies, as a concurrent stop can.
struct StoppedMeanwhile {
    sessions: Arc<ProviderSessions>,
    status: FakeStatus,
}

impl ProviderStatus for StoppedMeanwhile {
    fn mark_owned(&self, _session_id: &str) {}
    fn release_owned(&self, _session_id: &str) {}
    fn apply(&self, session_id: &str, status: ProviderSessionStatus) {
        self.sessions.unbind(session_id);
        self.status.apply(session_id, status);
    }
    fn release(&self, session_id: &str) {
        self.status.release(session_id);
    }
    fn emit_event(&self, _event: ProviderSessionEvent) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn a_turn_starting_as_its_session_stops_is_not_left_busy() {
    let sessions = Arc::new(ProviderSessions::default());
    bound(&sessions, "s1", 1);
    let status = StoppedMeanwhile {
        sessions: sessions.clone(),
        status: FakeStatus::default(),
    };
    let sink = NotificationSink::new(status, "chat", 1, sessions);
    sink.route(STATUS_NOTIFICATION, status_params("s1", "busy"));
    assert_eq!(sink.status.status.take(), ["Busy s1", "release s1"]);
}

#[tokio::test]
async fn a_handoff_without_a_runnable_command_returns_the_session_to_its_chat() {
    let runtime = FakeRuntime::running(&[("s1", "active")]);
    *runtime.handoff_answer.lock().unwrap() = Some(serde_json::json!({ "argv": [] }));
    assert!(handoff(&runtime, "s1").await.is_err());
    send(&runtime, "s1", "hi").await.unwrap();
    assert_eq!(
        runtime.sent(),
        [
            protocol::RESUME,
            protocol::HANDOFF,
            protocol::HANDBACK,
            protocol::SEND
        ]
    );
}

#[tokio::test]
async fn ended_sessions_are_those_ended_or_gone() {
    let runtime = FakeRuntime::running(&[("live", "active"), ("done", "archived")]);
    let ended = ended_sessions(
        &runtime,
        vec!["live".into(), "done".into(), "deleted".into()],
    )
    .await;
    assert_eq!(ended, ["done", "deleted"]);
}
