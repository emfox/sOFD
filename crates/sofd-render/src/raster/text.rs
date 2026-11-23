//! Painting text runs.

use super::*;

/// What a text run contributed to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOutcome {
    /// Every glyph the run named was drawn.
    Drawn,
    /// The run drew, but some of its glyphs had no outline in any face. The
    /// count is carried rather than a flag: one missing ideograph in a heading
    /// and a run of them are the same boolean and not the same problem.
    ShortGlyphs {
        /// Glyph positions that came out empty.
        glyphs: usize,
    },
    /// The run's face could not be loaded, so the run was skipped.
    NoFace,
    /// The run has no fill, only a stroke. Outlined text needs both to show,
    /// and neither painter can synthesize the missing one, so the run is
    /// skipped and counted rather than drawn as an invisible no-op.
    StrokeOnly,
}

pub(crate) fn draw_text(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    state: &State,
    view: Rect,
    fonts: &FontManager,
    options: &RenderOptions,
) -> Result<TextOutcome> {
    if run.glyphs.is_empty() {
        return Ok(TextOutcome::Drawn);
    }
    if run.fill.is_none() && run.stroke.is_some() {
        return Ok(TextOutcome::StrokeOnly);
    }
    // A face that will not load costs its run, not the page, and is counted in
    // `skipped_font_runs`. The chain gets one more chance, told what the run
    // is about to draw so the substitute can cover those characters.
    let wanted = run_wanted(run);
    let Some(face) = sofd_display::run_face_for(fonts, run, &wanted) else {
        return Ok(TextOutcome::NoFace);
    };
    draw_text_inner(pixmap, run, state, view, fonts, face, options)
}

/// The code points a run will draw, or nothing when its codes are glyph
/// indices. `text` is no help there: it serves search and copy, not drawing.
pub(crate) fn run_wanted(run: &GlyphRun) -> Vec<u32> {
    if run.codes_are_glyphs {
        return Vec::new();
    }
    run.glyphs.iter().map(|&c| u32::from(c)).collect()
}

pub(crate) fn draw_text_inner(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    state: &State,
    view: Rect,
    fonts: &FontManager,
    face: Arc<sofd_font::FontFace>,
    options: &RenderOptions,
) -> Result<TextOutcome> {
    let mut missing = 0usize;
    let wanted = run_wanted(run);
    for (&gid, offset) in run.glyphs.iter().zip(run.glyph_offsets()) {
        let Some((outline, _source)) =
            fonts.outline_for_code_in(&face, gid, run.codes_are_glyphs, &wanted)
        else {
            if fonts.is_missing_code(&face, gid, run.codes_are_glyphs) {
                missing += 1;
                tracing::debug!(
                    code = gid,
                    glyphs = run.codes_are_glyphs,
                    "a run code had no outline in its face or any fallback"
                );
            }
            continue;
        };
        // Outlines are normalized to 1/1000 em with y pointing up, and the
        // run's own matrix scales, turns and places them from there.
        let glyph = run.glyph_matrix(offset);
        let total = total_transform(state, &view, options.dpi);
        let ts_path = to_ts_path(&outline, total.pre_concat(to_ts(glyph)))?;
        if let Some(paint) = &run.fill {
            let p = solid_paint(paint);
            pixmap.fill_path(&ts_path, &p, FillRule::Winding, Transform::identity(), None);
        }
        if let Some(paint) = &run.stroke {
            let p = paint_for(paint, total).unwrap_or_else(|| solid_paint(paint));
            let mut ts_stroke = to_ts_stroke(&run.stroke_style, state, options.dpi);
            if is_hairline(&p, &ts_stroke) {
                ts_stroke.width = hairline_width(&ts_stroke);
            }
            pixmap.stroke_path(&ts_path, &p, &ts_stroke, Transform::identity(), None);
        }
    }
    Ok(if missing > 0 {
        TextOutcome::ShortGlyphs { glyphs: missing }
    } else {
        TextOutcome::Drawn
    })
}
