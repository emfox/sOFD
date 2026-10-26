//! The engine as an HTTP service: a store of open documents and a router that
//! turns them into PDF, PNG, SVG and verdicts. The binary is this plus the
//! settings it reads from the environment, so this is a caller mounting the
//! router in a process of their own.
//!
//! Run the example, POST a document to `/api/v1/documents`, then fetch its PDF.
use std::net::Ipv4Addr;

use sofd_server::{AppState, Store, api, tenancy::KeyRing};

/// The largest document this example accepts. A real deployment states it, the
/// store's capacity beside it, and both are what keep an upload from filling
/// memory rather than what make it work.
const MAX_BODY: usize = 256 * 1024 * 1024;
/// How many documents stay open at once.
const MAX_OPEN: usize = 8;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // An empty key ring means every caller is one shared tenant. A service on a
    // network names its keys instead, and `AppState::guarded` turns them on.
    let state = AppState::guarded(Store::new(MAX_OPEN), KeyRing::new());
    let app = api::router(state, MAX_BODY);

    // Loopback only: a trial run should not be a service anybody can reach. The
    // port is read from the environment so two runs, or a run and a test, do
    // not fight over one number; 8080 is what a reader expects to curl.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    println!("serving on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            // Ctrl+C stops the accept loop and lets in-flight work finish.
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
