//! JSON shapes the API returns.
//! No HTTP types live here, so every shape is testable on the host.

use serde::{Deserialize, Serialize};
use sofd::Document;
use sofd_error::OfdError;
use uuid::Uuid;

// The API answers with the same shapes the other bindings hand out, so a client
// generated from this schema and one reading the wasm objects see one set of
// field names.
pub use sofd_bindings::{
    DEFAULT_SEARCH, DocumentInfo, SearchHitData as SearchHit, SignatureData as SignatureStatus,
};

/// What an upload returns: the id every later route works on, and its facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    /// Id to pass to the other routes.
    pub id: Uuid,
    /// The document's declaration-level facts.
    pub info: DocumentInfo,
}

/// What the service says about itself on the health route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// Always `ok`: anything else means the process did not answer.
    pub status: &'static str,
    /// How many documents the service holds open.
    pub documents: usize,
    /// Crate version of the engine behind it.
    pub version: &'static str,
}

/// Search every page, returning the trimmed hits.
/// The options are fixed to match the other bindings: case-insensitive
/// substring matches, no cap on the hit count.
pub fn search_hits(doc: &Document, needle: &str) -> Vec<SearchHit> {
    sofd_bindings::search_hits(doc, needle, DEFAULT_SEARCH)
}

/// Verify every signature, in document order.
/// A document that declares no signatures is an error, matching the CLI.
pub fn signature_statuses(doc: &mut Document) -> Result<Vec<SignatureStatus>, OfdError> {
    sofd_bindings::signature_statuses(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The API and the wasm binding answer with one shape, so a generated
    /// client and a browser reading the objects see the same field names.
    #[test]
    fn the_api_shapes_are_the_bindings_shapes() {
        let hit = SearchHit {
            page: 2,
            snippet: "found".to_string(),
        };
        let json = serde_json::to_string(&hit).expect("serialize");
        assert_eq!(json, r#"{"page":2,"snippet":"found"}"#);
        let back: SearchHit = serde_json::from_str(&json).expect("a client reads it back");
        assert_eq!(back, hit);

        let status = SignatureStatus {
            location: "Signs/Sign_0/Signature.xml".to_string(),
            valid: true,
            unchecked: false,
            untrusted: false,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(json.contains("\"untrusted\":false"), "got {json}");
    }
}
