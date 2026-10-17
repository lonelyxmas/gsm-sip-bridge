// specs/096 User Story 3: every steady-state recovery the line supervisor
// performs is counted per line and per reason, so a route that keeps
// vanishing -- repaired every tick, never counted as flapping -- is visible
// without reading logs. Drives a real supervisor `Reporter` over a real
// control socket into the real registry, the same path
// `supervise::orchestrate`'s strongSwan steady loop uses.

use gsm_sip_bridge::control::protocol::{
    AgentKind, AgentState, ObservedEvent, TunnelRecoveryReason,
};
use gsm_sip_bridge::control::server::start_control_server;
use gsm_sip_bridge::metrics;
use gsm_sip_bridge::observability::reporter::Reporter;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

async fn wait_for<F: Fn() -> bool>(check: F, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if check() {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("condition not met within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn recoveries(module_id: &str, reason: TunnelRecoveryReason) -> f64 {
    metrics::VOWIFI_TUNNEL_RECOVERIES_TOTAL
        .with_label_values(&[module_id, reason.as_str()])
        .get()
}

fn tunnel_state() -> AgentState {
    AgentState {
        child_sa_up: Some(true),
        tunnel_flapping: Some(false),
        ..Default::default()
    }
}

fn recovered(reason: TunnelRecoveryReason) -> ObservedEvent {
    ObservedEvent::TunnelRecovered { reason }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_recoveries_are_counted_per_reason_and_never_rewind() {
    use TunnelRecoveryReason::{DefaultRouteMissing, PcscfUnreachable};

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir
        .path()
        .join("tunnel-recoveries-test.sock")
        .to_str()
        .unwrap()
        .to_string();

    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = start_control_server(&socket_path, cmd_tx, shutdown_rx).await;

    let module = "test-tunnel-recoveries";
    let interval = Duration::from_millis(50);
    let reporter = Reporter::spawn(
        socket_path.clone(),
        AgentKind::Supervisor,
        module.to_string(),
        interval,
    );

    reporter.report(
        tunnel_state(),
        vec![
            recovered(DefaultRouteMissing),
            recovered(DefaultRouteMissing),
            recovered(PcscfUnreachable),
        ],
    );
    wait_for(
        || {
            recoveries(module, DefaultRouteMissing) == 2.0
                && recoveries(module, PcscfUnreachable) == 1.0
        },
        Duration::from_secs(5),
    )
    .await;

    // Heartbeats and event-free reports carry state only: nothing to count.
    reporter.report(tunnel_state(), Vec::new());
    tokio::time::sleep(interval * 4).await;
    assert_eq!(recoveries(module, DefaultRouteMissing), 2.0);
    assert_eq!(recoveries(module, PcscfUnreachable), 1.0);

    // A restarted supervisor reports through a fresh reporter: the count
    // carries on rather than starting again from zero.
    drop(reporter);
    let restarted = Reporter::spawn(
        socket_path.clone(),
        AgentKind::Supervisor,
        module.to_string(),
        interval,
    );
    restarted.report(tunnel_state(), vec![recovered(DefaultRouteMissing)]);
    wait_for(
        || recoveries(module, DefaultRouteMissing) == 3.0,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(recoveries(module, PcscfUnreachable), 1.0);

    let _ = shutdown_tx.send(true);
    handle.abort();
}
