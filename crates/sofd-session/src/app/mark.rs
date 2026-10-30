//! Annotations: reading them, listing them and turning to them.

use super::*;

impl App {
    /// Whether the annotation panel is showing.
    pub fn annotations_open(&self) -> bool {
        self.annotations_open
    }

    /// Show or hide the annotation panel, reading the annotations when it
    /// first opens. Reading them walks the container, so it is done once and
    /// kept rather than on every frame.
    pub fn set_annotations_open(&mut self, open: bool) {
        self.annotations_open = open;
        if open && self.annotations.is_none() {
            self.load_annotations();
        }
    }

    /// Read every annotation in the document and remember them.
    /// A document with none is not a failure; it is a document with none, so
    /// the absence is an empty list rather than an error.
    pub fn load_annotations(&mut self) {
        let Some(doc) = self.doc.as_mut() else {
            self.annotations = Some(Vec::new());
            return;
        };
        self.annotations = Some(doc.annotations().unwrap_or_default());
    }

    /// Every annotation, in page order, or an empty list if none has been read.
    pub fn annotations(&self) -> &[Annotation] {
        self.annotations.as_deref().unwrap_or(&[])
    }

    /// How many annotations the open document carries, whether or not read.
    pub fn annotation_count(&self) -> usize {
        self.annotations().len()
    }

    /// Which annotation the reader is on, if any.
    pub fn annotation_index(&self) -> Option<usize> {
        self.annotation_index
    }

    /// The annotation the reader is on, if any.
    pub fn current_annotation(&self) -> Option<&Annotation> {
        self.annotation_index
            .and_then(|index| self.annotations().get(index))
    }

    /// Move to the next annotation, wrapping to the first.
    pub fn next_annotation(&mut self) {
        if self.annotations().is_empty() {
            return;
        }
        let next = match self.annotation_index {
            Some(index) => (index + 1) % self.annotations().len(),
            None => 0,
        };
        self.show_annotation(next);
    }

    /// Move to the previous annotation, wrapping to the last.
    pub fn previous_annotation(&mut self) {
        if self.annotations().is_empty() {
            return;
        }
        let previous = match self.annotation_index {
            Some(0) | None => self.annotations().len() - 1,
            Some(index) => index - 1,
        };
        self.show_annotation(previous);
    }

    /// Jump to an annotation, turning to the page it marks up: a note the
    /// reader cannot see is not a note they found.
    pub fn show_annotation(&mut self, index: usize) {
        let Some(annotation) = self.annotations().get(index) else {
            return;
        };
        let page = annotation.page;
        self.annotation_index = Some(index);
        self.go_to(page);
    }

    /// The annotations on one page, with a flag for the one being visited.
    /// The canvas paints the current one stronger, so this is derived state
    /// rather than something the window works out for itself.
    pub fn annotations_on(&self, page: usize) -> Vec<(&Annotation, bool)> {
        self.annotations()
            .iter()
            .enumerate()
            .filter(|(_, annotation)| annotation.page == page)
            .map(|(index, annotation)| (annotation, Some(index) == self.annotation_index))
            .collect()
    }

    /// The heading on the annotation panel.
    pub fn annotations_heading(&self) -> String {
        self.i18n.text(MessageId::AnnotPanelTitle)
    }

    /// How many annotations the document has, in the reader's language.
    pub fn annotation_count_label(&self) -> String {
        self.i18n.text_with(
            MessageId::AnnotCount,
            [("count", self.annotation_count().to_string().into())],
        )
    }

    /// One row of the annotation list: its page, its kind and its note.
    /// The row carries all three because a list of bare kinds would make the
    /// reader open each one to find out which is which.
    pub fn annotation_label(&self, index: usize) -> String {
        let Some(annotation) = self.annotations().get(index) else {
            return String::new();
        };
        let mut line = format!("{}  {}", annotation.page + 1, annotation.kind);
        if let Some(remark) = annotation.remark.as_ref().filter(|r| !r.is_empty()) {
            line.push_str("  ");
            line.push_str(remark);
        }
        line
    }

    /// The notice shown when the document carries no annotations.
    pub fn no_annotations_label(&self) -> String {
        self.i18n.text(MessageId::AnnotNone)
    }
}
