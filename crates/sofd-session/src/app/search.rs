//! Search: the query, the hits and turning to them.

use super::*;

impl App {
    /// Whether the search panel is showing.
    pub fn search_open(&self) -> bool {
        self.search_open
    }

    /// Show or hide the search panel. Hiding it keeps the query and the hits,
    /// so reopening the panel does not make the reader type it all again.
    pub fn set_search_open(&mut self, open: bool) {
        self.search_open = open;
    }

    /// The text being looked for.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replace the query and search for it. An empty query clears the results
    /// rather than matching everything: the reader who emptied the box is done
    /// searching, not asking for the whole document.
    pub fn set_query(&mut self, query: &str) {
        if self.query == query {
            return;
        }
        self.query = query.to_string();
        self.run_search();
    }

    /// Search for the current query, replacing the hits.
    pub fn run_search(&mut self) {
        self.hits.clear();
        self.hit_index = None;
        if self.query.is_empty() {
            return;
        }
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let options = SearchOptions::default();
        self.hits = doc
            .search(&self.query, options)
            .into_iter()
            .map(|hit| Hit {
                page: hit.page,
                snippet: hit.snippet,
                rect: hit.rect,
            })
            .collect();
        if !self.hits.is_empty() {
            self.hit_index = Some(0);
        }
    }

    /// Every hit, in page order.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Which hit the reader is on, if any.
    pub fn hit_index(&self) -> Option<usize> {
        self.hit_index
    }

    /// The hit the reader is on, if any.
    pub fn current_hit(&self) -> Option<&Hit> {
        self.hit_index.and_then(|index| self.hits.get(index))
    }

    /// Whether a real search ran and found nothing. An empty query is not a
    /// search, and neither is a query made before a document was open, so
    /// neither reports "no matches".
    pub fn no_match(&self) -> bool {
        !self.query.is_empty() && self.doc.is_some() && self.hits.is_empty()
    }

    /// Move to the next hit, wrapping to the first.
    pub fn next_hit(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        let next = match self.hit_index {
            Some(index) => (index + 1) % self.hits.len(),
            None => 0,
        };
        self.show_hit(next);
    }

    /// Move to the previous hit, wrapping to the last.
    pub fn previous_hit(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        let previous = match self.hit_index {
            Some(0) | None => self.hits.len() - 1,
            Some(index) => index - 1,
        };
        self.show_hit(previous);
    }

    /// Jump to a hit, turning to its page: a hit the reader cannot see is not
    /// a hit they found, so this moves the document as well as the selection.
    pub fn show_hit(&mut self, index: usize) {
        let Some(hit) = self.hits.get(index) else {
            return;
        };
        self.hit_index = Some(index);
        self.go_to(hit.page);
    }

    /// The hits on one page, and which of them is the current one. The canvas
    /// paints rectangles from this, so it is derived state rather than
    /// something the window works out for itself.
    pub fn hit_rects_on(&self, page: usize) -> Vec<(sofd_common::Rect, bool)> {
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| hit.page == page)
            .map(|(index, hit)| (hit.rect, Some(index) == self.hit_index))
            .collect()
    }

    /// The label on the toolbar toggle that opens the search panel.
    pub fn search_heading(&self) -> String {
        self.text(ReaderMessage::SearchTitle)
    }

    /// The heading inside the search panel. Distinct from the toggle: two
    /// controls wearing the same word on one screen make the reader guess
    /// which one they are looking at.
    pub fn search_panel_title(&self) -> String {
        self.text(ReaderMessage::SearchPanelTitle)
    }

    /// The placeholder in the search box.
    pub fn search_placeholder(&self) -> String {
        self.i18n.text(MessageId::SearchPlaceholder)
    }

    /// How many results the current query found, in the reader's language.
    pub fn search_count_label(&self) -> String {
        self.i18n.text_with(
            MessageId::SearchResultCount,
            [("count", self.hits.len().to_string().into())],
        )
    }

    /// The notice shown when a query found nothing.
    pub fn no_match_label(&self) -> String {
        self.text(ReaderMessage::NoMatch)
    }

    /// One row of the hit list: its page and the text around the match.
    /// The row says which page it is on because the list spans the document,
    /// and a row without a page number is a hit the reader must go find.
    pub fn hit_label(&self, index: usize, snippet: &str) -> String {
        let page = self.hits.get(index).map_or(0, |hit| hit.page + 1);
        format!("{page}  {snippet}")
    }
}
