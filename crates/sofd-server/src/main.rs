//! Process entry point: settings, tracing, listener, serve.
//! All logic lives in the library so tests can drive it in-process.

use std::process::ExitCode;

use sofd_server::api;
use sofd_server::{AppState, Drain, Settings, Store};

#[tokio::main]
async fn main() -> ExitCode {
    // A container's health check asks this process whether it can answer: the
    // route a caller would use, fetched from inside, so a container that
    // started but cannot serve is not reported healthy.
    if std::env::args().nth(1).as_deref() == Some("--health-check") {
        return match health_check().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("sofd-server: {message}");
                ExitCode::FAILURE
            }
        };
    }
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("sofd-server: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Ask the instance this process is configured to be whether it answers.
async fn health_check() -> Result<(), String> {
    let settings = Settings::from_env().map_err(|err| err.to_string())?;
    sofd_server::health::probe(settings.bind).await
}

/// Bind the configured address and serve until the process is asked to stop.
async fn run() -> Result<(), String> {
    let settings = Settings::from_env().map_err(|err| err.to_string())?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let store = match &settings.data_dir {
        Some(dir) => Store::persistent(settings.max_documents, dir)
            .map_err(|err| format!("cannot use {}: {err}", dir.display()))?,
        None => Store::new(settings.max_documents),
    };
    let authenticated = !settings.api_keys.is_open();
    let state = AppState::guarded(store, settings.api_keys.clone()).limited(settings.rate_limit);
    let app = api::router(state, settings.max_body_bytes);
    let listener = tokio::net::TcpListener::bind(settings.bind)
        .await
        .map_err(|err| format!("cannot bind {}: {err}", settings.bind))?;
    let local = listener
        .local_addr()
        .map_err(|err| format!("cannot read the bound address: {err}"))?;
    tracing::info!(address = %local, "sofd-server is listening");
    match settings.data_dir {
        Some(dir) => tracing::info!(directory = %dir.display(), "documents are persisted"),
        None => tracing::info!("documents are held in memory only"),
    }
    if authenticated {
        tracing::info!(tenants = settings.api_keys.len(), "API keys are required");
    } else {
        tracing::warn!("no API keys set, so every caller shares one tenant");
    }
    tracing::info!(
        rate = settings.rate_limit.rate,
        burst = settings.rate_limit.burst,
        "per-caller rate limit"
    );

    let drain = Drain::new();
    let serving = axum::serve(listener, app).with_graceful_shutdown(signal(drain.clone()));

    // A stop may come from a signal or from the process being asked to quit;
    // either way the socket has stopped accepting by the time the future
    // resolves, and only in-flight work is left. It gets the grace period.
    let serving = tokio::spawn(async move { serving.await.map_err(|e| e.to_string()) });
    drain.wait().await;
    tracing::info!(
        grace_seconds = settings.drain.duration().as_secs_f64(),
        "stopping; letting in-flight work finish"
    );
    let drained = tokio::time::timeout(settings.drain.duration(), serving)
        .await
        .map_err(|_| "in-flight work did not finish within the grace period".to_owned())?
        .map_err(|err| format!("the server task failed: {err}"))?;
    drained.map_err(|err| format!("the server stopped: {err}"))?;
    tracing::info!("stopped cleanly");
    Ok(())
}

/// Resolve once the process is asked to stop, having announced it on `drain`
/// so in-flight work is not cut off. A signal handler that cannot be installed
/// leaves the wait pending, which is what keeps the process serving.
async fn signal(drain: Drain) {
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(err) => {
            tracing::warn!(error = %err, "no SIGTERM handler; waiting for an interrupt");
            let _ = tokio::signal::ctrl_c().await;
            drain.stop();
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    drain.stop();
}
