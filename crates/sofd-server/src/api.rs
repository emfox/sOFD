//! The HTTP surface: routes, handlers and the query shapes they read. An upload
//! is the only route that takes a body; every other route works on the id it
//! returns. CPU-bound work goes to a blocking thread, never the async runtime.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sofd::{Document, RenderOptions};
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::dto::{self, DocumentInfo, Health, Opened, SearchHit, SignatureStatus};
use crate::error::ApiError;
use crate::ratelimit::{Caller, Verdict};
use crate::store::{self, Shared};
use crate::{AppState, Tenant};

/// Resolution used when a PNG request leaves `dpi` out.
const DEFAULT_DPI: f64 = 96.0;
/// Lowest resolution the service will rasterize at.
const MIN_DPI: f64 = 12.0;
/// Highest resolution the service will rasterize at. Print quality tops out
/// well below this; the real ceiling is the renderer's own pixel budget, which
/// an A4 page passes at 600 dpi and fails at 1200.
const MAX_DPI: f64 = 600.0;
/// How long one request may take before the service gives up on it. A whole
/// document converting to PDF is the slowest route, and this is set well past
/// what that costs so only a stuck request meets it.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The query a PNG render reads.
#[derive(Debug, Clone, Deserialize)]
pub struct PngQuery {
    /// Rasterization resolution, clamped to a sane range.
    pub dpi: Option<f64>,
}

/// The query a search reads.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    /// Text to look for; its absence is a bad request.
    pub needle: String,
}

/// Build the router the service runs. CORS is permissive because the callers
/// are browser front ends and OA portals; the body limit is what keeps an
/// upload from filling memory.
pub fn router(state: AppState, max_body_bytes: usize) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/openapi.json", get(openapi_document))
        .route("/api/v1/documents", post(open))
        .route("/api/v1/documents/{id}", get(info).delete(close))
        .route("/api/v1/documents/{id}/pdf", get(pdf))
        .route("/api/v1/documents/{id}/search", get(search))
        .route("/api/v1/documents/{id}/verify", get(verify))
        .route("/api/v1/documents/{id}/pages/{page}/png", get(png))
        .route("/api/v1/documents/{id}/pages/{page}/svg", get(svg))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(RequestBodyLimitLayer::new(max_body_bytes))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(state.clone(), throttle))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .with_state(state)
}

/// Hold each caller to its rate limit. The caller is the API key when one is
/// presented and the peer address otherwise, so an open service still tells
/// one client from another.
async fn throttle(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let caller = Caller::of(&request);
    match state.limiter.check(&caller, std::time::Instant::now()) {
        Verdict::Allow => next.run(request).await,
        Verdict::RetryAfter(wait) => {
            let seconds = wait.as_secs().max(1);
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, seconds.to_string())],
                Json(serde_json::json!({
                    "error": "too many requests; slow down and try again",
                })),
            )
                .into_response()
        }
    }
}

/// Report that the service is up, and how much it is holding.
async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        documents: state.store.len(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// The OpenAPI description of this service, so a client can be generated from
/// what the service itself says it does.
async fn openapi_document() -> Json<serde_json::Value> {
    Json(crate::openapi::document())
}

/// Open an uploaded document and keep it under a fresh id, owned by the
/// caller's tenant.
async fn open(
    State(state): State<AppState>,
    tenant: Tenant,
    body: Bytes,
) -> Result<(StatusCode, Json<Opened>), ApiError> {
    if state.store.is_full() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "too many open documents",
        ));
    }
    // `Bytes` is a refcounted handle, so the clone is what the store keeps
    // while the closure consumes the original to parse.
    let stored = body.clone();
    let opened = blocking(move || {
        let mut doc = Document::open_bytes(&body)?;
        let info = DocumentInfo::of(&mut doc);
        Ok::<_, sofd_error::OfdError>((doc, info))
    })
    .await??;
    let id = state.store.insert(tenant.id(), opened.0, &stored)?;
    Ok((StatusCode::CREATED, Json(Opened { id, info: opened.1 })))
}

/// Report the facts of an open document.
async fn info(
    State(state): State<AppState>,
    tenant: Tenant,
    Path(id): Path<Uuid>,
) -> Result<Json<DocumentInfo>, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let info = blocking(move || DocumentInfo::of(&mut store::lock(&doc))).await?;
    Ok(Json(info))
}

/// Release an open document.
async fn close(
    State(state): State<AppState>,
    tenant: Tenant,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state.store.remove(tenant.id(), id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::unknown_document(id))
    }
}

/// Rasterize one page and answer with PNG bytes.
async fn png(
    State(state): State<AppState>,
    tenant: Tenant,
    Path((id, page)): Path<(Uuid, usize)>,
    Query(query): Query<PngQuery>,
) -> Result<Response, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let dpi = query.dpi.unwrap_or(DEFAULT_DPI).clamp(MIN_DPI, MAX_DPI);
    let bytes = blocking(move || {
        let options = RenderOptions {
            dpi,
            ..Default::default()
        };
        // The merged outcome is the facade's: a page that lost a glyph still
        // answers, and the counts say so without this route merging them.
        store::lock(&doc)
            .png_outcome(page, &options)
            .map(|(bytes, _lost)| bytes)
    })
    .await??;
    Ok(png_response(bytes))
}

/// Convert one page and answer with an SVG document.
async fn svg(
    State(state): State<AppState>,
    tenant: Tenant,
    Path((id, page)): Path<(Uuid, usize)>,
) -> Result<Response, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let text = blocking(move || store::lock(&doc).to_svg(page)).await??;
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], text).into_response())
}

/// Convert the whole document and answer with PDF bytes.
async fn pdf(
    State(state): State<AppState>,
    tenant: Tenant,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let bytes = blocking(move || store::lock(&doc).to_pdf()).await??;
    Ok(([(header::CONTENT_TYPE, "application/pdf")], bytes).into_response())
}

/// Search every page of an open document.
async fn search(
    State(state): State<AppState>,
    tenant: Tenant,
    Path(id): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let hits = blocking(move || dto::search_hits(&store::lock(&doc), &query.needle)).await?;
    Ok(Json(hits))
}

/// Verify every signature of an open document.
async fn verify(
    State(state): State<AppState>,
    tenant: Tenant,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<SignatureStatus>>, ApiError> {
    let doc = shared(&state, &tenant, id)?;
    let statuses = blocking(move || dto::signature_statuses(&mut store::lock(&doc))).await??;
    Ok(Json(statuses))
}

/// Answer with PNG bytes under the image content type.
fn png_response(bytes: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "image/png")], bytes).into_response()
}

/// The document behind an id, if the caller's tenant owns it. An id owned by
/// another tenant answers as missing, so its existence does not leak.
fn shared(state: &AppState, tenant: &Tenant, id: Uuid) -> Result<Shared, ApiError> {
    match state.store.get(tenant.id(), id) {
        Ok(Some(doc)) => Ok(doc),
        Ok(None) => Err(ApiError::unknown_document(id)),
        Err(err) => Err(ApiError::storage(err)),
    }
}

/// Hand CPU-bound work to a blocking thread, where it belongs.
async fn blocking<T, F>(work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Ok(tokio::task::spawn_blocking(work).await?)
}
