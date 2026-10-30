//! The documents the service holds, and the copy each one keeps on disk.
//! Parsed state cannot be serialised; the bytes it came from can, so those are
//! what is persisted and the parsed value is rebuilt from them after a restart.
//! A mutex rather than the reader's session model, since a session is a window's
//! state and a service has no window; counts come from [`sofd::ConversionOutcome`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sofd::Document;
use uuid::Uuid;

/// One open document, shared by every request working on it.
pub type Shared = Arc<Mutex<Document>>;

/// An open document and the tenant that uploaded it.
struct Held {
    doc: Shared,
    tenant: String,
}

/// The open documents, backed by a directory of the files they came from.
#[derive(Clone)]
pub struct Store {
    open: Arc<Mutex<HashMap<Uuid, Held>>>,
    root: Option<PathBuf>,
    capacity: usize,
}

impl Store {
    /// A store that keeps only what is in memory. Documents do not survive a
    /// restart, which is what a test or a short-lived process wants.
    pub fn new(capacity: usize) -> Self {
        Self {
            open: Arc::new(Mutex::new(HashMap::new())),
            root: None,
            capacity,
        }
    }

    /// A store that writes each document under `root`, so an id opened before
    /// a restart can be reopened after one. The directory is created here, and
    /// documents already in it are adopted as open.
    pub fn persistent(capacity: usize, root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let store = Self {
            open: Arc::new(Mutex::new(HashMap::new())),
            root: Some(root),
            capacity,
        };
        store.adopt();
        Ok(store)
    }

    /// Whether this store writes documents to disk.
    pub fn is_persistent(&self) -> bool {
        self.root.is_some()
    }

    /// The directory documents are written under, when there is one.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Keep a document under a fresh id and hand the id back. In a persistent
    /// store the bytes are written first, so a visible document is one that
    /// survives a restart. No other tenant can read it back.
    pub fn insert(&self, tenant: &str, doc: Document, bytes: &[u8]) -> std::io::Result<Uuid> {
        let id = Uuid::new_v4();
        self.persist(id, tenant, bytes)?;
        self.lock().insert(
            id,
            Held {
                doc: Arc::new(Mutex::new(doc)),
                tenant: tenant.to_owned(),
            },
        );
        Ok(id)
    }

    /// The document behind an id, but only for the tenant that owns it. A
    /// document owned by someone else reads as absent, so a stranger cannot
    /// tell an id it may not see from one that does not exist.
    pub fn get(&self, tenant: &str, id: Uuid) -> std::io::Result<Option<Shared>> {
        if let Some(held) = self.lock().get(&id) {
            return Ok((held.tenant == tenant).then(|| Arc::clone(&held.doc)));
        }
        let Some(bytes) = self.read(id)? else {
            return Ok(None);
        };
        let owner = self.read_owner(id);
        if owner != tenant {
            return Ok(None);
        }
        let doc = Document::open_bytes(&bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
        let shared = Arc::new(Mutex::new(doc));
        self.lock().insert(
            id,
            Held {
                doc: Arc::clone(&shared),
                tenant: owner,
            },
        );
        Ok(Some(shared))
    }

    /// The tenant that owns an id, if any, without opening the document.
    pub fn owner_of(&self, id: Uuid) -> Option<String> {
        self.lock().get(&id).map(|held| held.tenant.clone())
    }

    /// Drop a document and its file, reporting whether the tenant owned it.
    pub fn remove(&self, tenant: &str, id: Uuid) -> bool {
        let owned = match self.lock().get(&id) {
            Some(held) => held.tenant == tenant,
            None => false,
        };
        if !owned {
            return false;
        }
        self.lock().remove(&id);
        if let Some(root) = &self.root {
            let _ = std::fs::remove_file(root.join(file_name(id)));
            let _ = std::fs::remove_file(root.join(owner_name(id)));
        }
        true
    }

    /// How many documents are open.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether no document is open.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the store is at its capacity, so opening has to be refused.
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity
    }

    /// Adopt the files already in the directory, so a restart reopens what an
    /// earlier process was holding rather than reporting every id unknown.
    fn adopt(&self) {
        let Some(root) = &self.root else { return };
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        let mut ids: Vec<Uuid> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().and_then(parse_file_name))
            .collect();
        // A stable order keeps the capacity cut deterministic across runs.
        ids.sort_unstable();
        let mut open = self.lock();
        for id in ids.into_iter().take(self.capacity) {
            let Ok(Some(bytes)) = self.read(id) else {
                continue;
            };
            if let Ok(doc) = Document::open_bytes(&bytes) {
                open.insert(
                    id,
                    Held {
                        doc: Arc::new(Mutex::new(doc)),
                        tenant: self.read_owner(id),
                    },
                );
            }
        }
    }

    /// Write the bytes of a document and the tenant that owns it. The owner
    /// travels in a sidecar so a restart keeps a document with its uploader
    /// rather than handing it to whoever asks first.
    fn persist(&self, id: Uuid, tenant: &str, bytes: &[u8]) -> std::io::Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        std::fs::write(root.join(file_name(id)), bytes)?;
        std::fs::write(root.join(owner_name(id)), tenant)
    }

    /// The tenant recorded with a document on disk, or [`ANONYMOUS`] when the
    /// sidecar is missing, which is what a store written before owners were
    /// recorded looks like.
    fn read_owner(&self, id: Uuid) -> String {
        let Some(root) = &self.root else {
            return crate::tenancy::ANONYMOUS.to_owned();
        };
        std::fs::read_to_string(root.join(owner_name(id)))
            .map(|text| text.trim().to_owned())
            .ok()
            .filter(|owner| !owner.is_empty())
            .unwrap_or_else(|| crate::tenancy::ANONYMOUS.to_owned())
    }

    /// Read the bytes of a document from disk, if there are any.
    fn read(&self, id: Uuid) -> std::io::Result<Option<Vec<u8>>> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        match std::fs::read(root.join(file_name(id))) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// The poisoned-lock fallback: a panic mid-insert leaves the map usable,
    /// and refusing every later request over it would be worse.
    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Held>> {
        self.open.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// The file a document is stored as, named for its id.
fn file_name(id: Uuid) -> String {
    format!("{id}.ofd")
}

/// The sidecar recording which tenant owns a document.
fn owner_name(id: Uuid) -> String {
    format!("{id}.owner")
}

/// The id a stored file name stands for. A sidecar is not one, so it is
/// skipped when the directory is scanned.
fn parse_file_name(name: &str) -> Option<Uuid> {
    let stem = name.strip_suffix(".ofd")?;
    Uuid::parse_str(stem).ok()
}

/// Lock a document for work, the same way [`Store`] locks its map.
pub fn lock(doc: &Shared) -> MutexGuard<'_, Document> {
    doc.lock().unwrap_or_else(|err| err.into_inner())
}
