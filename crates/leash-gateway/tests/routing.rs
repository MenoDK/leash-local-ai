//! Routing, residency and the circuit breaker, through the public API only.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test code: a failed expectation is the failure report

use std::sync::Arc;
use std::time::Duration;

use leash_censor::{Limits, Provenance, SafeEnvelope, seal};
use leash_gateway::{
    Behaviour, CircuitState, Gateway, GatewayError, Locality, Options, StandIn, Step, SystemPrompt,
};

const COOLDOWN: Duration = Duration::from_secs(30);

fn envelope(provenance: Provenance) -> SafeEnvelope {
    seal(
        "quarterly figures: 1200, 1350, 1410",
        provenance,
        Limits::default(),
    )
}

fn system() -> SystemPrompt {
    SystemPrompt::new("Summarise the data.")
}

fn standin(id: &str, locality: Locality) -> Arc<StandIn> {
    Arc::new(StandIn::new(id, locality))
}

fn gateway(threshold: u32, providers: &[&Arc<StandIn>]) -> Gateway {
    let mut gw = Gateway::new(threshold, COOLDOWN);
    for p in providers {
        gw.register(Arc::<StandIn>::clone(p));
    }
    gw
}

#[tokio::test(start_paused = true)]
async fn confidential_text_never_reaches_a_remote_provider() {
    let remote = standin("remote", Locality::Remote);
    let gw = gateway(3, &[&remote]);

    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Confidential),
            &Options::default(),
        )
        .await;

    assert_eq!(routed.answer, Err(GatewayError::NoLocalProvider));
    assert_eq!(
        remote.calls(),
        0,
        "the remote provider must not have been asked"
    );
    assert_eq!(routed.trace.len(), 1);
    assert_eq!(routed.trace[0].step, Step::SkippedRemote);
}

#[tokio::test(start_paused = true)]
async fn confidential_text_is_answered_locally_even_when_a_remote_comes_first() {
    let remote = standin("remote", Locality::Remote);
    let local = standin("local", Locality::Local);
    let gw = gateway(3, &[&remote, &local]);

    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Confidential),
            &Options::default(),
        )
        .await;

    assert!(routed.answer.is_ok());
    assert_eq!((remote.calls(), local.calls()), (0, 1));
    assert_eq!(routed.trace[0].step, Step::SkippedRemote);
    assert_eq!(routed.trace[1].step, Step::Answered);
}

#[tokio::test(start_paused = true)]
async fn a_failing_local_provider_does_not_make_confidential_text_fall_back_to_remote() {
    let local = standin("local", Locality::Local);
    let remote = standin("remote", Locality::Remote);
    local.set_behaviour(Behaviour::Failing);
    let gw = gateway(3, &[&local, &remote]);

    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Confidential),
            &Options::default(),
        )
        .await;

    assert_eq!(routed.answer, Err(GatewayError::Unavailable));
    assert_eq!(
        remote.calls(),
        0,
        "fail closed: the remote provider is not a fallback for confidential text"
    );
}

#[tokio::test(start_paused = true)]
async fn an_open_local_circuit_does_not_send_confidential_text_elsewhere() {
    let local = standin("local", Locality::Local);
    let remote = standin("remote", Locality::Remote);
    local.set_behaviour(Behaviour::Failing);
    let gw = gateway(2, &[&local, &remote]);
    for _ in 0..2 {
        let _ = gw
            .complete(
                &system(),
                &envelope(Provenance::Confidential),
                &Options::default(),
            )
            .await;
    }
    assert_eq!(gw.status()[0].circuit, CircuitState::Open);

    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Confidential),
            &Options::default(),
        )
        .await;

    assert_eq!(routed.answer, Err(GatewayError::Unavailable));
    assert_eq!(routed.trace[0].step, Step::SkippedCircuitOpen);
    assert_eq!((local.calls(), remote.calls()), (2, 0));
}

#[tokio::test(start_paused = true)]
async fn ordinary_text_falls_through_a_failing_provider_and_the_trace_says_so() {
    let first = standin("first", Locality::Local);
    let second = standin("second", Locality::Remote);
    first.set_behaviour(Behaviour::Failing);
    let gw = gateway(3, &[&first, &second]);

    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Document),
            &Options::default(),
        )
        .await;

    let answer = routed.answer.expect("the second provider answers");
    assert_eq!(answer.model, "stand-in:second");
    assert!(matches!(routed.trace[0].step, Step::Failed { .. }));
    assert_eq!(routed.trace[1].step, Step::Answered);
}

#[tokio::test(start_paused = true)]
async fn the_circuit_stops_calls_to_a_provider_that_keeps_failing() {
    let only = standin("only", Locality::Local);
    only.set_behaviour(Behaviour::Failing);
    let gw = gateway(3, &[&only]);

    for _ in 0..10 {
        let _ = gw
            .complete(
                &system(),
                &envelope(Provenance::Document),
                &Options::default(),
            )
            .await;
    }
    assert_eq!(
        only.calls(),
        3,
        "three failures open the circuit; the other seven calls never reach the provider"
    );
}

#[tokio::test(start_paused = true)]
async fn after_the_cooldown_only_one_of_many_concurrent_calls_is_the_probe() {
    let only = standin("only", Locality::Local);
    only.set_behaviour(Behaviour::Failing);
    let gw = Arc::new(gateway(1, &[&only]));
    let _ = gw
        .complete(
            &system(),
            &envelope(Provenance::Document),
            &Options::default(),
        )
        .await;
    assert_eq!(only.calls(), 1);

    only.set_behaviour(Behaviour::Slow(Duration::from_secs(5)));
    tokio::time::advance(COOLDOWN).await;

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..25 {
        let gw = Arc::clone(&gw);
        tasks.spawn(async move {
            gw.complete(
                &system(),
                &envelope(Provenance::Document),
                &Options::default(),
            )
            .await
        });
    }
    let mut answered = 0;
    while let Some(done) = tasks.join_next().await {
        if done.expect("task").answer.is_ok() {
            answered += 1;
        }
    }
    assert_eq!(
        only.calls(),
        2,
        "one failure before, exactly one probe after"
    );
    assert_eq!(answered, 1);
    assert_eq!(
        gw.status()[0].circuit,
        CircuitState::Closed,
        "the probe succeeded"
    );
}

#[tokio::test(start_paused = true)]
async fn a_provider_that_hangs_is_cut_off_and_counted_as_failing() {
    let slow = standin("slow", Locality::Local);
    slow.set_behaviour(Behaviour::Slow(Duration::from_secs(600)));
    let gw = gateway(2, &[&slow]);
    let options = Options {
        timeout: Duration::from_secs(2),
        ..Options::default()
    };

    let routed = gw
        .complete(&system(), &envelope(Provenance::Document), &options)
        .await;
    assert!(
        matches!(&routed.trace[0].step, Step::Failed { error } if error.contains("no answer within"))
    );

    let _ = gw
        .complete(&system(), &envelope(Provenance::Document), &options)
        .await;
    assert_eq!(gw.status()[0].circuit, CircuitState::Open);
}

#[tokio::test(start_paused = true)]
async fn an_abandoned_call_does_not_hold_the_probe_forever() {
    let only = standin("only", Locality::Local);
    only.set_behaviour(Behaviour::Failing);
    let gw = Arc::new(gateway(1, &[&only]));
    let _ = gw
        .complete(
            &system(),
            &envelope(Provenance::Document),
            &Options::default(),
        )
        .await;
    only.set_behaviour(Behaviour::Slow(Duration::from_secs(600)));
    tokio::time::advance(COOLDOWN).await;

    // The probe starts, then the caller gives up on it.
    let (prompt, input, options) = (system(), envelope(Provenance::Document), Options::default());
    let call = gw.complete(&prompt, &input, &options);
    let abandoned = tokio::time::timeout(Duration::from_secs(1), call).await;
    assert!(abandoned.is_err());
    assert_eq!(
        gw.status()[0].circuit,
        CircuitState::Open,
        "an abandoned probe counts as a failure"
    );

    only.set_behaviour(Behaviour::Healthy);
    tokio::time::advance(COOLDOWN).await;
    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Document),
            &Options::default(),
        )
        .await;
    assert!(
        routed.answer.is_ok(),
        "the next cooldown allows a new probe"
    );
}

#[tokio::test]
async fn with_no_providers_the_error_says_so() {
    let gw = Gateway::new(3, COOLDOWN);
    let routed = gw
        .complete(
            &system(),
            &envelope(Provenance::Document),
            &Options::default(),
        )
        .await;
    assert_eq!(routed.answer, Err(GatewayError::NoProvider));
}
