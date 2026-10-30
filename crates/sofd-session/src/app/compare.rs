//! Comparing two revisions of one file. The comparison itself is the engine's
//! `Document::compare`; this holds the second revision and the verdict, so the
//! window can list what changed.

use super::*;
use sofd::{Comparison, PageChange};

impl App {
    /// Whether the comparison panel is showing.
    pub fn comparison_open(&self) -> bool {
        self.comparison_open
    }

    /// Show or hide the comparison panel.
    pub fn set_comparison_open(&mut self, open: bool) {
        self.comparison_open = open;
    }

    /// Compare the open document against a second revision held in memory.
    /// The second file is opened, compared and dropped: only its display list
    /// is needed, so it never becomes the document the reader is showing.
    pub fn compare_bytes(&mut self, bytes: &[u8]) -> Result<(), AppError> {
        let Some(current) = self.doc.as_ref() else {
            return Err(AppError {
                message: "no document is open to compare".to_string(),
            });
        };
        let other = Document::open_bytes(bytes).map_err(|err| AppError {
            message: err.to_string(),
        })?;
        self.comparison = Some(current.compare(&other));
        self.comparison_open = true;
        Ok(())
    }

    /// The verdict on the last comparison, if one was run.
    pub fn comparison(&self) -> Option<&Comparison> {
        self.comparison.as_ref()
    }

    /// The heading on the comparison panel.
    pub fn comparison_heading(&self) -> String {
        self.i18n.text(MessageId::ComparePanelTitle)
    }

    /// The one-line summary: identical, or how many pages moved.
    pub fn comparison_summary(&self) -> String {
        let Some(comparison) = self.comparison.as_ref() else {
            return String::new();
        };
        if comparison.is_identical() {
            return self.i18n.text(MessageId::CompareIdentical);
        }
        self.i18n.text_with(
            MessageId::CompareChanged,
            [("count", comparison.changed_pages().to_string().into())],
        )
    }

    /// One row of the change list: the page and what changed on it. `page` is
    /// the page index, so a row and the page it turns to are the same number.
    pub fn comparison_row(&self, page: usize) -> String {
        let Some(diff) = self.comparison.as_ref().and_then(|c| c.pages.get(page)) else {
            return String::new();
        };
        self.i18n.text_with(
            MessageId::ComparePage,
            [
                ("page", (diff.index + 1).to_string().into()),
                ("change", change_name(diff.change).into()),
            ],
        )
    }

    /// The pages that changed, as rows for the window to list.
    pub fn comparison_changes(&self) -> Vec<(usize, PageChange)> {
        self.comparison
            .as_ref()
            .map(|comparison| {
                comparison
                    .pages
                    .iter()
                    .filter(|page| page.change != PageChange::Unchanged)
                    .map(|page| (page.index, page.change))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A stable lower-case name for a change, so the row reads the same in any
/// language the catalogue is not yet translated into.
fn change_name(change: PageChange) -> &'static str {
    match change {
        PageChange::Unchanged => "unchanged",
        PageChange::Added => "added",
        PageChange::Removed => "removed",
        PageChange::Resized => "resized",
        PageChange::Text => "text",
        PageChange::Graphics => "graphics",
    }
}
