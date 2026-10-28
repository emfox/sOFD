//! The HTTP delivery path: a document service over the `core` facade. Uploading
//! an `.ofd` file yields a document id; every other route works on that id, so
//! a caller renders page after page without resending the file.

#![warn(missing_docs)]

pub mod api;
pub mod config;
pub mod drain;
pub mod dto;
pub mod error;
pub mod health;
pub mod openapi;
pub mod ratelimit;
pub mod store;
pub mod tenancy;

pub use config::{BadSetting, Settings};
pub use drain::{Deadline, Drain};
pub use error::ApiError;
pub use ratelimit::{Caller, Limiter, RateLimit, Verdict};
pub use store::Store;
pub use tenancy::{KeyRing, Tenant};

/// Everything a handler is given: the documents and who may see them.
#[derive(Clone)]
pub struct AppState {
    /// The open documents.
    pub store: Store,
    /// The keys the service accepts.
    pub keys: KeyRing,
    /// How often each caller may ask for work.
    pub limiter: Limiter,
}

impl AppState {
    /// State with no authentication and a generous limit, which is what a
    /// single-tenant service and the test suite use.
    pub fn open(store: Store) -> Self {
        Self {
            store,
            keys: KeyRing::new(),
            limiter: Limiter::new(RateLimit::generous()),
        }
    }

    /// State that requires one of `keys` to be presented.
    pub fn guarded(store: Store, keys: KeyRing) -> Self {
        Self {
            store,
            keys,
            limiter: Limiter::new(RateLimit::generous()),
        }
    }

    /// State holding every caller to `limit`.
    pub fn limited(mut self, limit: RateLimit) -> Self {
        self.limiter = Limiter::new(limit);
        self
    }
}
