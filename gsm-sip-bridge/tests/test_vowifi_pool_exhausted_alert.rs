// specs/097: a supervisor's `PcscfPoolExhausted` rebuilds, sent with the
// P-CSCFs the agent gave up on, reach the daemon over the real control socket
// and are counted under their own reason. The episode and its alert decision
// are unit-tested in `metrics::ingest` with an injected config, since the
// category's minimum threshold (30s) is too long to wait out here.

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

fn exhausted_rebuilds(module_id: &str) -> f64 {
    metrics::VOWIFI_TUNNEL_RECOVERIES_TOTAL
        .with_label_values(&[module_id, TunnelRecoveryReason::PcscfPoolExhausted.as_str()])
        .get()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_exhausted_rebuilds_are_counted_under_their_own_reason() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir
        .path()
        .join("pool-exhausted-test.sock")
        .to_str()
        .unwrap()
        .to_string();

    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = start_control_server(&socket_path, cmd_tx, shutdown_rx).await;

    let module = "test-pool-exhausted";
    let interval = Duration::from_millis(50);
    let reporter = Reporter::spawn(
        socket_path.clone(),
        AgentKind::Supervisor,
        module.to_string(),
        interval,
    );
    let rebuild = || ObservedEvent::TunnelRecovered {
        reason: TunnelRecoveryReason::PcscfPoolExhausted,
        pcscfs: vec!["2001:db8::11".to_string()],
        rebuilt_at_unix: Some(1_790_000_000),
        dead_peer: None,
    };
    let state = AgentState {
        child_sa_up: Some(true),
        tunnel_flapping: Some(false),
        ..Default::default()
    };

    reporter.report(state, vec![rebuild()]);
    reporter.report(state, vec![rebuild()]);
    wait_for(|| exhausted_rebuilds(module) == 2.0, Duration::from_secs(5)).await;

    // The probe's reason is a different series: the two rebuilds stay
    // distinguishable, which is the evidence the probe-removal decision needs.
    assert_eq!(
        metrics::VOWIFI_TUNNEL_RECOVERIES_TOTAL
            .with_label_values(&[module, TunnelRecoveryReason::PcscfUnreachable.as_str()])
            .get(),
        0.0
    );

    let _ = shutdown_tx.send(true);
    handle.abort();
}
