//! Assembling one PDF file out of the pages a document converted.
//!
//! `sofd-convert` writes a single page and stops there; the `/Pages` tree and
//! the `/Catalog` over them turn page objects into a file, and belong here.

#[cfg(feature = "pdf")]
use lopdf::{Dictionary, Document, Object, ObjectId};
#[cfg(feature = "pdf")]
use sofd_error::{ConvertError, OfdError, Result};

/// Wire the catalog and page tree, then serialize.
///
/// `page_ids` is in the order the pages should be read, and a page may appear
/// more than once: the same object is referenced twice rather than written
/// twice, which is what a repeated selection asks for.
#[cfg(feature = "pdf")]
pub(crate) fn finish_document(doc: &mut Document, page_ids: Vec<ObjectId>) -> Result<Vec<u8>> {
    let kids: Vec<Object> = page_ids.iter().copied().map(Object::Reference).collect();
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(kids));
    pages.set("Count", page_ids.len() as i64);
    let pages_id = doc.add_object(pages);
    for id in &page_ids {
        if let Ok(dict) = doc.get_dictionary_mut(*id) {
            dict.set("Parent", Object::Reference(pages_id));
        }
    }
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| {
        OfdError::Convert(ConvertError::MappingFailed {
            page: 0,
            object: "document".to_string(),
            reason: e.to_string(),
        })
    })?;
    Ok(out)
}
