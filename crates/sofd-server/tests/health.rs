//! The health check a container's `HEALTHCHECK` runs.
//!
//! It is covered here rather than by running it by hand, because the failure it
//! exists to catch — a container that started but cannot serve — is one nobody
//! sees unless the check is exercised from both sides.

use std::time::Duration;

use sofd_server::api;
use sofd_server::health::probe;
use sofd_server::{AppState, Store};

/// Start the service on `bind` and return the address it actually holds.
async fn serving_on(bind: &str) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind(bind).await.expect("bind");
    let address = listener.local_addr().expect("an address");
    let app = api::router(AppState::open(Store::new(1)), 1 << 20);
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("the server runs");
    });
    address
}

#[tokio::test]
async fn a_serving_instance_answers() {
    let address = serving_on("127.0.0.1:0").await;
    assert_eq!(probe(address).await, Ok(()));
}

#[tokio::test]
async fn a_port_with_nothing_on_it_fails() {
    // Bound and dropped, so the number is almost certainly free: the check has
    // to report a refusal rather than pass because something else answered.
    let free = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = free.local_addr().expect("an address");
    drop(free);
    let answer = probe(address).await;
    assert!(answer.is_err(), "a closed port answered: {answer:?}");
}

#[tokio::test]
async fn a_wildcard_bind_is_reached_through_the_check() {
    // The shape a container runs with. Linux accepts a connection to `0.0.0.0`
    // itself, so this alone does not prove the translation; `reachable`'s own
    // test does. It pins that the check works on the address a container binds.
    let address = serving_on("0.0.0.0:0").await;
    let mut unspecified = address;
    unspecified.set_ip(std::net::Ipv4Addr::UNSPECIFIED.into());
    assert_eq!(probe(unspecified).await, Ok(()));
}

#[tokio::test]
async fn a_server_that_stops_fails_the_check() {
    // The case a rolling deploy relies on: once the port is closed the check
    // stops passing, so a container past its grace period is replaced.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("an address");
    let app = api::router(AppState::open(Store::new(1)), 1 << 20);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .expect("the server runs");
    });
    assert_eq!(probe(address).await, Ok(()));
    stop.send(()).expect("ask the server to stop");
    serving.await.expect("serving ends");
    // The socket needs a moment to close after the future resolves.
    let mut answer = probe(address).await;
    for _ in 0..50 {
        if answer.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        answer = probe(address).await;
    }
    assert!(
        answer.is_err(),
        "a stopped server still answered: {answer:?}"
    );
}
