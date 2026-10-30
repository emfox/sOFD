//! What a whole-document conversion lost, in one place.
//!
//! One page at a time is what the converters produce, so a caller converting
//! many pages would otherwise merge counts itself, and every caller that did
//! would merge them differently. The merge belongs here, next to the loop it
//! summarizes.

/// What converting a run of pages cost in fidelity.
///
/// A count is the only sign a conversion degraded: the file still writes, so
/// a caller reads this rather than guessing from the output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversionOutcome {
    /// Pages the conversion was asked for.
    pub pages: usize,
    /// Glyph positions that resolved to no outline, so the output is short of
    /// marks even though every page is there.
    pub missing_glyphs: usize,
    /// Images left out because their bytes would not decode or their size was
    /// rejected.
    pub missing_images: usize,
    /// Runs the target format could not carry as text and dropped.
    pub dropped_runs: usize,
    /// Faces the document asked for and this build could not supply.
    pub missing_fonts: usize,
}

impl ConversionOutcome {
    /// Whether every page came out as the document states it.
    pub fn is_lossless(&self) -> bool {
        self.missing_glyphs == 0
            && self.missing_images == 0
            && self.dropped_runs == 0
            && self.missing_fonts == 0
    }

    /// Fold another converted page into this outcome. Pages add up, and so do
    /// the losses: two pages short one glyph each are two glyphs short.
    pub fn merge(&mut self, other: &Self) {
        self.pages += other.pages;
        self.missing_glyphs += other.missing_glyphs;
        self.missing_images += other.missing_images;
        self.dropped_runs += other.dropped_runs;
        self.missing_fonts += other.missing_fonts;
    }

    /// Fold in one page's vector-conversion counts.
    pub(crate) fn absorb_stats(&mut self, stats: &sofd_convert::ConversionStats) {
        self.missing_glyphs += stats.missing_glyphs;
        self.dropped_runs += stats.dropped_runs;
        self.missing_fonts += stats.missing_fonts.len();
    }
}
