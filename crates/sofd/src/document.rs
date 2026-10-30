//! The document handle every capability hangs off.

use std::collections::{BTreeSet, HashMap};

use sofd_common::Rect;
use sofd_display::{DisplayList, DrawOp, PageDisplay};
use sofd_error::Result;
use sofd_font::{FontId, FontManager};
use sofd_io::ByteSource;
use sofd_model::document::Document as ModelDocument;
use sofd_package::Package;
use sofd_parser::{ParsedDocument, Parser};

/// One page's vector-conversion counts, folded into a whole-run outcome.
#[cfg(feature = "convert")]
fn fold(stats: sofd_convert::ConversionStats, pages: usize) -> crate::ConversionOutcome {
    let mut outcome = crate::ConversionOutcome {
        pages,
        ..Default::default()
    };
    outcome.absorb_stats(&stats);
    outcome
}

/// An opened OFD document: pages flattened and container fonts registered,
/// so every capability works against the same prepared state.
pub struct Document {
    package: Package,
    parsed: ParsedDocument,
    list: DisplayList,
    fonts: FontManager,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("pages", &self.list.page_count)
            .field("fonts", &self.fonts)
            .finish()
    }
}

impl Document {
    /// Open a document from any byte source.
    pub fn open(source: &dyn ByteSource) -> Result<Self> {
        let mut package = Package::open(source)?;
        let mut parser = Parser::new(&mut package);
        let parsed = parser.open_document()?;
        let page_count = parsed.document.pages.len();
        let mut pages = Vec::with_capacity(page_count);
        for index in 0..page_count {
            pages.push(parser.page_display(&parsed, index)?);
        }
        let mut list = DisplayList { page_count, pages };
        let mut fonts = FontManager::new();
        let doc_dir = parsed.doc_dir.clone();
        let registered = register_container_fonts(&mut package, &doc_dir, &mut fonts, &list)?;
        remap_fonts(&mut list, &registered);
        // The chain is built last, once every face the document declares has
        // been registered, so it covers all of them rather than the first.
        fonts.build_fallback_chain();
        Ok(Self {
            package,
            parsed,
            list,
            fonts,
        })
    }

    /// Open a document from bytes, the common case for uploads and tests.
    pub fn open_bytes(data: &[u8]) -> Result<Self> {
        let source = sofd_io::SliceSource::new(data, "document");
        Self::open(&source)
    }

    /// Pages the document declares.
    pub fn page_count(&self) -> usize {
        self.list.page_count
    }

    /// One flattened page, or `None` when the index is out of range.
    pub fn page(&self, index: usize) -> Option<&PageDisplay> {
        self.list.page(index)
    }

    /// The whole flattened display list.
    pub fn display_list(&self) -> &DisplayList {
        &self.list
    }

    /// Compare this revision against another, page by page. Both sides are
    /// already flattened, so the comparison is over what each draws rather
    /// than over the markup it draws it from.
    pub fn compare(&self, other: &Document) -> crate::Comparison {
        crate::compare::compare(&self.list.pages, &other.list.pages)
    }

    /// Faces registered from the container.
    pub fn fonts(&self) -> &FontManager {
        &self.fonts
    }

    /// Faces this document asks for and this build could not supply. Filled by
    /// rendering, since a face is only searched for when a page needs it.
    pub fn missing_fonts(&self) -> Vec<sofd_font::Unresolved> {
        self.fonts.unresolved()
    }

    /// Mutable access to the font manager, for callers that bring their own
    /// faces and want to register them under the ids the list already uses.
    pub fn fonts_mut(&mut self) -> &mut FontManager {
        &mut self.fonts
    }

    /// The box a page is drawn on, in millimetres, taken from the document's
    /// own page area. Callers that only want the size use this rather than
    /// reaching into the model for it.
    pub fn page_box(&self) -> Rect {
        self.parsed.document.common_data.page_area.physical_box
    }

    /// The parsed document model, for callers that need the raw structure.
    pub fn model(&self) -> &ModelDocument {
        &self.parsed.document
    }

    /// Version the document declares, such as `1.0`.
    /// The minor part is appended only when present.
    pub fn version(&self) -> String {
        let version = &self.parsed.ofd.version;
        match &version.sub {
            Some(sub) if !sub.is_empty() => format!("{}.{}", version.major, sub),
            _ => version.major.clone(),
        }
    }

    /// The container, for signature and archival work that reads entries.
    pub fn package(&mut self) -> &mut Package {
        &mut self.package
    }

    /// Rasterize one page at the given options.
    #[cfg(feature = "render")]
    pub fn render_page(
        &self,
        index: usize,
        options: &sofd_render::RenderOptions,
    ) -> Result<sofd_render::RasterOutput> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        sofd_render::render_page(page, &self.fonts, options)
    }

    /// Walk one page into geometry, in page millimetres, without rasterizing
    /// it. A shell with a canvas of its own draws this rather than a bitmap,
    /// so a zoom costs a transform instead of another render.
    #[cfg(feature = "render")]
    pub fn vector_page(
        &self,
        index: usize,
        options: &sofd_render::RenderOptions,
    ) -> Result<sofd_render::VectorPage> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        Ok(sofd_render::page_to_vector(page, &self.fonts, options))
    }

    /// Rasterize one page and encode it as PNG, reporting what the page was
    /// short of. The face search only happens while drawing, so a caller that
    /// needs to warn about a missing font has to ask here and not before.
    #[cfg(feature = "convert")]
    pub fn to_png_with_stats(
        &self,
        index: usize,
        options: &sofd_render::RenderOptions,
    ) -> Result<(Vec<u8>, sofd_render::RasterOutput)> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        let raster = sofd_render::render_page(page, &self.fonts, options)?;
        let png = sofd_convert::png::encode_png(&raster, index)?;
        Ok((png, raster))
    }

    /// Convert one page to SVG, with the conversion's own counts.
    #[cfg(feature = "convert")]
    pub fn to_svg_with_stats(
        &self,
        index: usize,
    ) -> Result<(String, sofd_convert::ConversionStats)> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        sofd_convert::page_to_svg_with_stats(
            page,
            &sofd_convert::SvgOptions {
                fonts: Some(&self.fonts),
                ..Default::default()
            },
        )
    }

    /// Convert one page to PDF, with the conversion's own counts.
    #[cfg(feature = "pdf")]
    pub fn to_pdf_with_stats(
        &self,
        index: usize,
    ) -> Result<(Vec<u8>, sofd_convert::ConversionStats)> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        sofd_convert::page_to_pdf_with_stats(
            page,
            &sofd_convert::PdfOptions {
                fonts: Some(&self.fonts),
                ..Default::default()
            },
        )
    }

    /// Rasterize several pages, in parallel when the async feature is on.
    #[cfg(feature = "render")]
    pub fn render_pages(
        &self,
        indices: &[usize],
        options: &sofd_render::RenderOptions,
    ) -> Vec<Result<sofd_render::RasterOutput>> {
        #[cfg(feature = "async")]
        {
            use rayon::prelude::*;
            indices
                .par_iter()
                .map(|&index| self.render_page(index, options))
                .collect()
        }
        #[cfg(not(feature = "async"))]
        {
            indices
                .iter()
                .map(|&index| self.render_page(index, options))
                .collect()
        }
    }

    /// Plan a print job over the whole document.
    #[cfg(feature = "print")]
    pub fn print_job(&self, settings: &sofd_print::PrintSettings) -> Result<sofd_print::PrintJob> {
        sofd_print::plan(&self.list, settings)
    }

    /// Plan and rasterize a print job, in print order.
    #[cfg(feature = "print")]
    pub fn print_render(
        &self,
        settings: &sofd_print::PrintSettings,
    ) -> Result<Vec<sofd_print::RasterPage>> {
        let job = self.print_job(settings)?;
        sofd_print::render_job(&self.list, &self.fonts, &job)
    }

    /// Convert the whole document to one PDF file, with the conversion's
    /// counts. The fonts go with it, or a run has no outline to draw and falls
    /// back to live text that only reads back where the face is installed.
    #[cfg(feature = "pdf")]
    pub fn to_pdf_with_stats_list(&self) -> Result<(Vec<u8>, sofd_convert::ConversionStats)> {
        let every: Vec<usize> = self.list.pages.iter().map(|page| page.index).collect();
        self.to_pdf_pages_with_stats(&every)
    }

    /// Convert the whole document to one PDF file.
    #[cfg(feature = "pdf")]
    pub fn to_pdf(&self) -> Result<Vec<u8>> {
        Ok(self.to_pdf_with_stats_list()?.0)
    }

    /// Convert the named pages to one PDF file, in the order given. The
    /// selection is a list rather than a span because a reader's pages are not
    /// contiguous: facing mode shows a pair, and a search lands anywhere.
    #[cfg(feature = "pdf")]
    pub fn to_pdf_pages(&self, pages: &[usize]) -> Result<Vec<u8>> {
        Ok(self.to_pdf_pages_with_stats(pages)?.0)
    }

    /// Convert the named pages to one PDF file, with the conversion's counts.
    /// Each page keeps its own physical box in the one document.
    ///
    /// The loop is here rather than in `sofd-convert` because a page is what
    /// that crate converts: it writes one page into a document, and the tree
    /// over the pages — order, parents, catalog — is this layer's to build.
    #[cfg(feature = "pdf")]
    pub fn to_pdf_pages_with_stats(
        &self,
        pages: &[usize],
    ) -> Result<(Vec<u8>, sofd_convert::ConversionStats)> {
        let options = sofd_convert::PdfOptions {
            fonts: Some(&self.fonts),
            ..Default::default()
        };
        let mut stats = sofd_convert::ConversionStats::default();
        let mut doc = lopdf::Document::new();
        let mut page_ids = Vec::with_capacity(pages.len());
        for wanted in pages {
            let page = self
                .page(*wanted)
                .ok_or_else(|| out_of_range(*wanted, self.page_count()))?;
            let mut sink = sofd_convert::PdfSink::new(page.page_box, &options);
            sofd_convert::replay(page, &mut sink)?;
            stats.absorb(sink.stats());
            page_ids.push(sink.emit_page(&mut doc)?);
        }
        Ok((crate::export::finish_document(&mut doc, page_ids)?, stats))
    }

    /// Convert the named pages to one PDF, with one merged account of what
    /// the conversion lost across all of them.
    #[cfg(feature = "pdf")]
    pub fn pdf_outcome(&self, pages: &[usize]) -> Result<(Vec<u8>, crate::ConversionOutcome)> {
        let (bytes, stats) = self.to_pdf_pages_with_stats(pages)?;
        Ok((bytes, fold(stats, pages.len())))
    }

    /// Convert one page to PNG, with one merged account of what it lost.
    #[cfg(all(feature = "render", feature = "convert"))]
    pub fn png_outcome(
        &self,
        index: usize,
        options: &sofd_render::RenderOptions,
    ) -> Result<(Vec<u8>, crate::ConversionOutcome)> {
        let (png, raster) = self.to_png_with_stats(index, options)?;
        let mut outcome = crate::ConversionOutcome {
            pages: 1,
            missing_glyphs: raster.missing_glyphs,
            missing_images: raster.skipped_image_ops,
            ..Default::default()
        };
        outcome.missing_fonts = self.missing_fonts().len();
        Ok((png, outcome))
    }

    /// Convert one page to SVG, with one merged account of what it lost.
    #[cfg(feature = "convert")]
    pub fn svg_outcome(&self, index: usize) -> Result<(String, crate::ConversionOutcome)> {
        let (svg, stats) = self.to_svg_with_stats(index)?;
        Ok((svg, fold(stats, 1)))
    }

    /// Convert every page to SVG, one standalone document per page, in order.
    ///
    /// Each page is a file of its own rather than one file of many pages: SVG
    /// has no page tree, so "the document as SVG" is the pages it holds.
    #[cfg(feature = "convert")]
    pub fn to_svg_pages(&self) -> Result<Vec<String>> {
        let options = sofd_convert::SvgOptions {
            fonts: Some(&self.fonts),
            ..Default::default()
        };
        self.list
            .pages
            .iter()
            .map(|page| sofd_convert::page_to_svg(page, &options))
            .collect()
    }

    /// Convert one page to SVG.
    #[cfg(feature = "convert")]
    pub fn to_svg(&self, index: usize) -> Result<String> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        sofd_convert::page_to_svg(page, &sofd_convert::SvgOptions::default())
    }

    /// Rasterize one page and encode it as PNG.
    #[cfg(feature = "convert")]
    pub fn to_png(&self, index: usize, options: &sofd_render::RenderOptions) -> Result<Vec<u8>> {
        let page = self
            .page(index)
            .ok_or_else(|| out_of_range(index, self.page_count()))?;
        sofd_convert::png::page_to_png(page, &self.fonts, options)
    }

    /// Read the signature list the document points at.
    #[cfg(feature = "sign")]
    pub fn signatures(&mut self) -> Result<Vec<sofd_sign::SignRef>> {
        let loc = self
            .parsed
            .ofd
            .doc_bodies
            .first()
            .and_then(|body| body.signatures.as_ref())
            .map(|loc| loc.value.clone())
            .ok_or_else(no_signatures)?;
        sofd_sign::read_sign_list(&mut self.package, &loc)
    }

    /// What each signature covers, and what the container holds outside every
    /// signature's scope. Clause 18.2.2: a signature protects the files its
    /// references name, so this is how a carrier sees what arrived afterwards.
    #[cfg(feature = "sign")]
    pub fn signature_coverage(&mut self) -> Result<Vec<sofd_sign::Coverage>> {
        let loc = self
            .parsed
            .ofd
            .doc_bodies
            .first()
            .and_then(|body| body.signatures.as_ref())
            .map(|loc| loc.value.clone())
            .ok_or_else(no_signatures)?;
        sofd_sign::read_coverage(&mut self.package, &loc)
    }

    /// Verify every signature in the list, in document order.
    #[cfg(feature = "sign")]
    pub fn verify_signatures(&mut self) -> Result<Vec<sofd_sign::VerifyStatus>> {
        let refs = self.signatures()?;
        refs.iter()
            .map(|sig| {
                let signature = sofd_sign::read_signature(&mut self.package, &sig.base_loc)?;
                sofd_sign::verify_signature(&mut self.package, &signature)
            })
            .collect()
    }

    /// Whether each signature carries a trusted timestamp, and whether the
    /// token really binds that signature. Clause 18.2.4: the proof hangs off
    /// the signed value, so a token that covers other bytes proves nothing.
    #[cfg(feature = "sign")]
    pub fn verify_timestamps(&mut self) -> Result<Vec<sofd_sign::TimestampStatus>> {
        let refs = self.signatures()?;
        refs.iter()
            .map(|sig| {
                let signature = sofd_sign::read_signature(&mut self.package, &sig.base_loc)?;
                sofd_sign::verify_timestamp(&signature)
            })
            .collect()
    }

    /// Everything worth showing about each signature: who signed, when their
    /// certificate runs, and how many files the proof covers.
    /// [`verify_signatures`](Self::verify_signatures) answers whether the proof
    /// holds; this answers what it is about, which is what a reader asks next.
    /// The invoice seal's text comes from the certificate, not the container.
    #[cfg(feature = "sign")]
    pub fn signature_details(&mut self) -> Result<Vec<sofd_sign::details::SignatureDetails>> {
        let refs = self.signatures()?;
        // Coverage is read once for the whole list: it walks the container,
        // and asking per signature would walk it once per signature.
        let list_loc = self
            .parsed
            .ofd
            .doc_bodies
            .first()
            .and_then(|body| body.signatures.as_ref())
            .map(|loc| loc.value.clone());
        let coverage = list_loc
            .and_then(|loc| sofd_sign::details::coverage_summary(&mut self.package, &loc).ok());
        let mut out = Vec::with_capacity(refs.len());
        for entry in &refs {
            let mut signature = sofd_sign::read_signature(&mut self.package, &entry.base_loc)?;
            signature.read_check_value(&mut self.package)?;
            let certificate = signature
                .certificate
                .as_deref()
                .and_then(|der| sofd_crypto::Certificate::parse_der(der).ok());
            let mut described =
                sofd_sign::details::describe(&signature, certificate.as_ref(), coverage.as_ref());
            // The descriptor does not state its own path, so a details list
            // whose entries all read as `None` would say nothing about which
            // signature is which; the entry that named this file does.
            described.id = described.id.or_else(|| Some(entry.base_loc.clone()));
            out.push(described);
        }
        Ok(out)
    }

    /// The seals a document draws, each resolved onto the page it sits on.
    /// Clause 18.2.3: a stamp names its page by id, so the id is joined to the
    /// page index here, and the seal image travels ready to draw.
    #[cfg(feature = "sign")]
    pub fn seals(&mut self) -> Result<Vec<Seal>> {
        let refs = self.signatures()?;
        let by_id = page_ids(&self.parsed);
        let mut out = Vec::new();
        for sign in &refs {
            let signature = sofd_sign::read_signature(&mut self.package, &sign.base_loc)?;
            let placements = sofd_sign::read_seals(&mut self.package, &sign.base_loc, &signature)?;
            for placement in placements {
                let Some(&page) = by_id.get(&placement.page_ref) else {
                    continue;
                };
                out.push(Seal {
                    page,
                    id: placement.id,
                    boundary: placement.boundary,
                    clip: placement.clip,
                    image: placement.image,
                    sealed: placement.sealed,
                });
            }
        }
        Ok(out)
    }

    /// Start an editing session over a copy of the document model.
    #[cfg(feature = "edit")]
    pub fn edit(&self, options: sofd_edit::EditOptions) -> sofd_edit::EditSession {
        sofd_edit::EditSession::new(self.parsed.document.clone(), options)
    }

    /// Full-text search over the flattened pages.
    #[cfg(feature = "search")]
    pub fn search(
        &self,
        needle: &str,
        options: sofd_search::SearchOptions,
    ) -> Vec<sofd_search::SearchHit> {
        let index = sofd_search::SearchIndex::build_with_fonts(&self.list, &self.fonts);
        index.search(needle, options)
    }

    /// Audit the container for archival readiness.
    #[cfg(feature = "archive")]
    pub fn audit(&mut self) -> Result<sofd_archive::Report> {
        sofd_archive::audit(&mut self.package)
    }

    /// One annotation, as a reader needs it: which page it marks up, what kind
    /// of mark it is and where its appearance sits.
    #[cfg(feature = "annot")]
    pub fn annotations(&mut self) -> Result<Vec<Annotation>> {
        let resolved = sofd_parser::marks::read_annotations(&mut self.package, &self.parsed)?;
        Ok(resolved
            .into_iter()
            .map(|entry| Annotation {
                page: entry.page,
                id: entry.annot.id,
                kind: entry.annot.kind,
                remark: entry.annot.remark,
                parameters: entry.annot.parameters,
                visible: entry.annot.visible,
                boundary: entry.annot.boundary,
            })
            .collect())
    }
}

/// A seal resolved onto the page it sits on, ready for a reader to draw.
#[cfg(feature = "sign")]
#[derive(Debug, Clone, PartialEq)]
pub struct Seal {
    /// Zero-based page index the seal is drawn on.
    pub page: usize,
    /// Identifier written in the document.
    pub id: String,
    /// Where on the page the seal sits, in page space.
    pub boundary: sofd_common::Rect,
    /// Region the seal is clipped to, when one is declared.
    pub clip: Option<sofd_common::Rect>,
    /// The seal image bytes, empty when none was named or found.
    pub image: Vec<u8>,
    /// The seal the signature carries inside itself, when it carries one.
    ///
    /// This is where an invoice's seal actually is. GB/T 38540 keeps it in
    /// `SES_Signature.toSign.eseal`, and its picture is usually an entire OFD
    /// document with the seal drawn as text, so a reader draws that document
    /// rather than laying a seal out from a specification.
    pub sealed: Option<sofd_sign::SealPicture>,
}

/// An annotation resolved onto the page it belongs to.
#[cfg(feature = "annot")]
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Zero-based page index the annotation marks up.
    pub page: usize,
    /// Identifier written in the document.
    pub id: String,
    /// What kind of mark it is, as written.
    pub kind: String,
    /// Free-text note, when the document carries one.
    pub remark: Option<String>,
    /// Name/value pairs from `Parameters`.
    pub parameters: Vec<(String, String)>,
    /// Whether the document says the mark is drawn.
    pub visible: bool,
    /// Where the mark's appearance sits on the page, in page space.
    pub boundary: Option<sofd_common::Rect>,
}

/// Page id to zero-based page index, in document order.
#[cfg(feature = "sign")]
fn page_ids(parsed: &sofd_parser::ParsedDocument) -> std::collections::HashMap<String, usize> {
    parsed
        .document
        .pages
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.id.clone(), index))
        .collect()
}

/// Register the fonts the document declares, keyed by the id the display list
/// references. A face named without a file keeps its declaration's traits; an
/// id never declared at all points at the generic family.
fn register_container_fonts(
    package: &mut Package,
    doc_dir: &str,
    fonts: &mut FontManager,
    list: &DisplayList,
) -> Result<HashMap<u32, FontId>> {
    let mut map: HashMap<u32, FontId> = HashMap::new();
    let used = used_font_ids(list);
    let glyphs = used_glyphs(list);
    let declared = sofd_parser::fonts::load_fonts(package, doc_dir)?;
    for id in &used {
        let Some(resource) = declared.get(id) else {
            continue;
        };
        if !resource.embedded {
            continue;
        }
        let family = if resource.family.is_empty() {
            "embedded".to_string()
        } else {
            resource.family.clone()
        };
        // Only the glyphs this document draws are worth carrying: a face can
        // be an order of magnitude smaller without them, and nothing outside
        // the display list can be asked for at render time.
        let data = match glyphs.get(id).and_then(|set| set.as_ref()) {
            Some(set) => {
                std::sync::Arc::new(sofd_font::subset(&resource.data, set.iter().copied()))
            }
            None => resource.data.clone(),
        };
        let registered =
            fonts
                .registry_mut()
                .register_embedded(resource.path.clone(), family, data);
        map.insert(*id, registered);
    }

    // Referenced but not embedded: each keeps its own declaration, since
    // clause 11.1's traits are what a host search matches on and two faces
    // that differ only in weight are not one face.
    let declared_by_id: HashMap<u32, sofd_parser::fonts::FontResource> = declared;
    for id in used {
        if map.contains_key(&id) {
            continue;
        }
        let registered = match declared_by_id.get(&id) {
            Some(resource) => fonts
                .registry_mut()
                .register_unshipped(resource.family.clone(), resource.traits.clone()),
            None => fonts.registry_mut().register_system(SUBSTITUTE_FAMILY),
        };
        map.insert(id, registered);
    }
    Ok(map)
}

/// Family used when a document references a font id it never declares.
/// Nothing about the face is known, so the generic alias is all there is.
const SUBSTITUTE_FAMILY: &str = "sans-serif";

/// Every font id the display list actually references, sorted.
fn used_font_ids(list: &DisplayList) -> Vec<u32> {
    let mut used = BTreeSet::new();
    for page in &list.pages {
        for op in &page.ops {
            if let DrawOp::Text(run) = op {
                used.insert(run.font.0);
            }
        }
    }
    used.into_iter().collect()
}

/// The glyphs each font id is asked to draw, so a face can be subset to them.
/// A run carrying code points cannot name its glyphs without the face, so that
/// font is marked `None` and left whole rather than guessed at.
fn used_glyphs(list: &DisplayList) -> HashMap<u32, Option<BTreeSet<u16>>> {
    let mut used: HashMap<u32, Option<BTreeSet<u16>>> = HashMap::new();
    for page in &list.pages {
        for op in &page.ops {
            let DrawOp::Text(run) = op else {
                continue;
            };
            let entry = used
                .entry(run.font.0)
                .or_insert_with(|| Some(BTreeSet::new()));
            match entry {
                // Code points: without the face the glyph ids are unknown.
                _ if !run.codes_are_glyphs => *entry = None,
                Some(set) => set.extend(run.glyphs.iter().copied()),
                None => {}
            }
        }
    }
    used
}

/// Rewrite font ids in the display list through the pairing.
fn remap_fonts(list: &mut DisplayList, map: &HashMap<u32, FontId>) {
    for page in &mut list.pages {
        for op in &mut page.ops {
            if let DrawOp::Text(run) = op
                && let Some(&id) = map.get(&run.font.0)
            {
                run.font = id;
            }
        }
    }
}

/// The page index was out of range.
#[cfg(any(feature = "render", feature = "convert", feature = "pdf"))]
fn out_of_range(index: usize, total: usize) -> sofd_error::OfdError {
    sofd_error::OfdError::Format(sofd_error::FormatError::PageOutOfRange { index, total })
}

/// The document declares no signatures at all.
#[cfg(feature = "sign")]
fn no_signatures() -> sofd_error::OfdError {
    sofd_error::OfdError::Format(sofd_error::FormatError::MissingSignatures)
}
