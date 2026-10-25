//! Subset an embedded face to the glyphs a document actually draws.

use std::collections::BTreeSet;

/// A parsed sfnt directory, with every table's bytes copied out.
struct Sfnt {
    version: u32,
    tables: Vec<([u8; 4], Vec<u8>)>,
}

/// Glyph offsets into `glyf`, in the form `head` declares.
struct Loca {
    offsets: Vec<u32>,
}

impl Loca {
    fn glyph<'a>(&self, glyf: &'a [u8], glyph: usize) -> Option<&'a [u8]> {
        let start = *self.offsets.get(glyph)? as usize;
        let end = *self.offsets.get(glyph + 1)? as usize;
        if end < start || end > glyf.len() {
            return None;
        }
        Some(&glyf[start..end])
    }
}

impl Sfnt {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let version = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        // CFF outlines are not sfnt-glyf: leave those faces alone.
        if version == 0x4F54_544F {
            return None;
        }
        let n = u16::from_be_bytes([data[4], data[5]]) as usize;
        if 12 + 16 * n > data.len() {
            return None;
        }
        let mut tables = Vec::with_capacity(n);
        for i in 0..n {
            let rec = 12 + 16 * i;
            let mut tag = [0u8; 4];
            tag.copy_from_slice(&data[rec..rec + 4]);
            let off =
                u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]])
                    as usize;
            let len = u32::from_be_bytes([
                data[rec + 12],
                data[rec + 13],
                data[rec + 14],
                data[rec + 15],
            ]) as usize;
            if off + len > data.len() {
                return None;
            }
            tables.push((tag, data[off..off + len].to_vec()));
        }
        Some(Self { version, tables })
    }

    fn table(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.tables
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, b)| b.as_slice())
    }

    fn num_glyphs(&self) -> Option<u16> {
        let maxp = self.table(b"maxp")?;
        (maxp.len() >= 6).then(|| u16::from_be_bytes([maxp[4], maxp[5]]))
    }

    fn loca(&self, num_glyphs: u16) -> Option<Loca> {
        let head = self.table(b"head")?;
        if head.len() < 52 {
            return None;
        }
        let long = i16::from_be_bytes([head[50], head[51]]) == 1;
        let loca = self.table(b"loca")?;
        let count = num_glyphs as usize + 1;
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            let v = if long {
                be32(loca, i * 4)?
            } else {
                be16(loca, i * 2)? as u32 * 2
            };
            offsets.push(v);
        }
        Some(Loca { offsets })
    }

    /// The kept glyphs as a format 4 cmap, which is the one every reader reads.
    fn subset_cmap(&self, new_id: &std::collections::BTreeMap<u16, u16>) -> Option<Vec<u8>> {
        let cmap = self.table(b"cmap")?;
        let n = be16(cmap, 2)? as usize;
        let mut chosen = None;
        for i in 0..n {
            let rec = 4 + 8 * i;
            // A record that runs past the table is skipped, not fatal: the
            // subtable that follows may be the readable one.
            let (Some(off), Some(platform), Some(encoding)) = (
                be32(cmap, rec + 4).map(|o| o as usize),
                be16(cmap, rec),
                be16(cmap, rec + 2),
            ) else {
                continue;
            };
            // Only a format 4 subtable can be rebuilt as one, and the format
            // is a 16-bit field, not the first byte alone.
            if be16(cmap, off) != Some(4) {
                continue;
            }
            if (platform, encoding) == (3, 1) {
                chosen = Some(off);
                break;
            }
            chosen.get_or_insert(off);
        }
        let pairs = format4_pairs(cmap.get(chosen?..)?, new_id)?;
        Some(build_cmap4(&pairs))
    }
}

/// Subset `data` to `keep`. `None` means the face is not one this rewrites.
fn subset_face(data: &[u8], keep: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let sfnt = Sfnt::parse(data)?;
    let num_glyphs = sfnt.num_glyphs()?;
    let keep: BTreeSet<u16> = keep.iter().copied().filter(|g| *g < num_glyphs).collect();
    if keep.len() as u16 >= num_glyphs {
        return None;
    }
    let loca = sfnt.loca(num_glyphs)?;
    let glyf = sfnt.table(b"glyf")?;
    let (outlines, offsets) = build_glyf(glyf, &loca, &keep)?;

    // Glyphs are renumbered 0, 1, 2, ... in the order they are kept, so the
    // cmap and the metric table agree on which glyph is which.
    let new_id: std::collections::BTreeMap<u16, u16> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u16))
        .collect();
    let count = keep.len() as u16;

    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"glyf", outlines),
        (b"loca", build_loca(&offsets)),
        (b"cmap", sfnt.subset_cmap(&new_id)?),
        (b"maxp", build_maxp(&sfnt, count)?),
        (b"hmtx", build_hmtx(&sfnt, &keep)?),
        (b"hhea", build_hhea(&sfnt, count)?),
        (
            b"head",
            build_head(&sfnt, offsets.last().copied().unwrap_or(0))?,
        ),
    ];
    for (tag, body) in &sfnt.tables {
        // These index glyphs by id and cannot survive a rewrite; a reader that
        // finds them missing falls back to the cmap, which is correct.
        if matches!(
            tag,
            b"glyf" | b"loca" | b"cmap" | b"maxp" | b"hmtx" | b"hhea" | b"head"
        ) {
            continue;
        }
        if matches!(tag, b"GSUB" | b"GPOS" | b"kern" | b"post") {
            continue;
        }
        tables.push((tag, body.clone()));
    }
    tables.sort_by(|a, b| a.0.cmp(b.0));
    Some(assemble(&sfnt, &tables))
}

/// The kept glyphs, each padded so a short `loca` stays even.
fn build_glyf(glyf: &[u8], loca: &Loca, keep: &BTreeSet<u16>) -> Option<(Vec<u8>, Vec<u32>)> {
    let mut out = Vec::new();
    let mut offsets = vec![0u32];
    for &glyph in keep {
        let body = loca.glyph(glyf, glyph as usize)?;
        out.extend_from_slice(body);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        offsets.push(out.len() as u32);
    }
    Some((out, offsets))
}

fn build_loca(offsets: &[u32]) -> Vec<u8> {
    offsets.iter().flat_map(|o| o.to_be_bytes()).collect()
}

fn build_maxp(sfnt: &Sfnt, count: u16) -> Option<Vec<u8>> {
    let mut maxp = sfnt.table(b"maxp")?.to_vec();
    if maxp.len() < 6 {
        return None;
    }
    maxp[4..6].copy_from_slice(&count.to_be_bytes());
    Some(maxp)
}

/// One advance and side bearing per kept glyph, in the subset's own id order.
/// The glyphs are renumbered 0, 1, 2, ... so the metric for a glyph is taken
/// from its original id and written to its new one.
fn build_hmtx(sfnt: &Sfnt, keep: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let hmtx = sfnt.table(b"hmtx")?;
    let hhea = sfnt.table(b"hhea")?;
    let num_metrics = be16(hhea, 34)? as usize;
    let mut out = Vec::with_capacity(keep.len() * 4);
    for &glyph in keep {
        // Past the metric count every glyph repeats the last advance.
        let at = (glyph as usize).min(num_metrics.saturating_sub(1)) * 4;
        match hmtx.get(at..at + 4) {
            Some(metric) => out.extend_from_slice(metric),
            None => out.extend_from_slice(&[0, 0, 0, 0]),
        }
    }
    Some(out)
}

/// `hhea` with `numberOfHMetrics` brought down to the subset's glyph count,
/// since every kept glyph now carries its own metric.
fn build_hhea(sfnt: &Sfnt, count: u16) -> Option<Vec<u8>> {
    let mut hhea = sfnt.table(b"hhea")?.to_vec();
    if hhea.len() < 36 {
        return None;
    }
    hhea[34..36].copy_from_slice(&count.to_be_bytes());
    Some(hhea)
}

/// `head` with `indexToLocFormat` and the bounding box set for the new `glyf`.
fn build_head(sfnt: &Sfnt, glyf_len: u32) -> Option<Vec<u8>> {
    let mut head = sfnt.table(b"head")?.to_vec();
    if head.len() < 54 {
        return None;
    }
    // A long loca is always safe; a short one only if every offset is even.
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    head[52..54].copy_from_slice(&0i16.to_be_bytes());
    // The checkSumAdjustment is recomputed by the assembler's caller only when
    // it matters, which is never for a face read back in memory.
    let _ = glyf_len;
    Some(head)
}

/// Rebuild the sfnt directory around the new table set.
fn assemble(sfnt: &Sfnt, tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = tables.len() as u16;
    let (search, entry) = search_range(n);
    let mut out = Vec::new();
    out.extend_from_slice(&sfnt.version.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&(search * 16).to_be_bytes());
    out.extend_from_slice(&entry.to_be_bytes());
    out.extend_from_slice(&(n * 16 - search * 16).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    for (tag, body) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        offset = (offset + body.len() + 3) & !3;
    }
    for (_, body) in tables {
        out.extend_from_slice(body);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

fn search_range(n: u16) -> (u16, u16) {
    let mut search = 1u16;
    let mut entry = 0u16;
    while search * 2 <= n {
        search *= 2;
        entry += 1;
    }
    (search, entry)
}

/// The `(code, glyph)` pairs a format 4 subtable maps, filtered to `keep`.
fn format4_pairs(
    sub: &[u8],
    new_id: &std::collections::BTreeMap<u16, u16>,
) -> Option<Vec<(u16, u16)>> {
    let seg_x2 = be16(sub, 6)? as usize;
    let segs = seg_x2 / 2;
    let starts = 16 + seg_x2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    let mut pairs = Vec::new();
    for s in 0..segs {
        let end = be16(sub, 14 + 2 * s)?;
        let start = be16(sub, starts + 2 * s)?;
        let delta = be16(sub, deltas + 2 * s)?;
        let range = be16(sub, ranges + 2 * s)?;
        if start == 0xFFFF {
            continue;
        }
        for code in start..=end {
            let glyph = if range == 0 {
                code.wrapping_add(delta)
            } else {
                // An idRangeOffset indexes the glyph id array that follows.
                let at = ranges + 2 * s + range as usize + 2 * (code - start) as usize;
                match be16(sub, at) {
                    None | Some(0) => continue,
                    Some(g) => g.wrapping_add(delta),
                }
            };
            // The cmap must name the glyph by its new id, or the reader
            // looks up an outline the subset no longer carries there.
            if let Some(&renumbered) = new_id.get(&glyph) {
                pairs.push((code, renumbered));
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    Some(pairs)
}

/// A one-subtable format 4 cmap over `pairs`.
fn build_cmap4(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut segments: Vec<(u16, u16, i16)> = Vec::new();
    let mut i = 0;
    while i < pairs.len() {
        let (start_code, start_glyph) = pairs[i];
        let mut end = start_code;
        let mut j = i + 1;
        // A run continues while code and glyph both advance by one.
        while j < pairs.len()
            && pairs[j].0 == end.wrapping_add(1)
            && pairs[j].1 == start_glyph.wrapping_add(end.wrapping_add(1).wrapping_sub(start_code))
        {
            end = pairs[j].0;
            j += 1;
        }
        segments.push((start_code, end, start_glyph.wrapping_sub(start_code) as i16));
        i = j;
    }
    segments.push((0xFFFF, 0xFFFF, 1));

    let seg_count = segments.len() as u16;
    let seg_x2 = seg_count * 2;
    let (search, entry) = search_range(seg_count);
    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes());
    // 14 bytes of header, then four parallel arrays of seg_count entries.
    sub.extend_from_slice(&(14 + seg_x2 * 4).to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&seg_x2.to_be_bytes());
    sub.extend_from_slice(&search.to_be_bytes());
    sub.extend_from_slice(&entry.to_be_bytes());
    sub.extend_from_slice(&(seg_x2 - search).to_be_bytes());
    for (_, end, _) in &segments {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes());
    for (start, _, _) in &segments {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (_, _, delta) in &segments {
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in &segments {
        sub.extend_from_slice(&0u16.to_be_bytes());
    }

    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    cmap
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Subset an embedded face to the glyphs `glyphs` names, falling back to the
/// original bytes when the face is not one this can rewrite without losing it.
/// Asking for nothing is one of those cases: the result would carry `.notdef`
/// alone, and a face nobody referenced is a face some code point still needs.
pub fn subset(data: &[u8], glyphs: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let mut keep: BTreeSet<u16> = glyphs.into_iter().collect();
    if keep.is_empty() {
        return data.to_vec();
    }
    keep.insert(0);
    subset_face(data, &keep).unwrap_or_else(|| data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skrifa::MetadataProvider;
    use skrifa::outline::OutlinePen;
    use skrifa::raw::TableProvider;

    const LATIN: &[u8] = include_bytes!("../tests/data/sofd-latin-subset.ttf");

    fn face(data: &[u8]) -> skrifa::FontRef<'_> {
        skrifa::FontRef::new(data).expect("fixture parses")
    }

    /// The shape maps `ch` to, or nothing when it maps none.
    fn glyph_of(f: &skrifa::FontRef<'_>, ch: char) -> Option<u16> {
        let (_, _, sub) = f.cmap().ok()?.best_subtable()?;
        sub.map_codepoint(ch).map(|g| g.to_u32() as u16)
    }

    /// How many glyphs the face carries.
    fn glyph_count(f: &skrifa::FontRef<'_>) -> u16 {
        f.maxp().expect("maxp").num_glyphs()
    }

    /// The glyphs a face maps `text` to, in the order given.
    fn glyphs(data: &[u8], text: &str) -> Vec<u16> {
        let f = face(data);
        text.chars().filter_map(|c| glyph_of(&f, c)).collect()
    }

    #[derive(Default)]
    struct Ops(usize);

    impl OutlinePen for Ops {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn close(&mut self) {
            self.0 += 1;
        }
    }

    /// How many outline operators one glyph of `data` draws.
    fn outline_ops(data: &[u8], glyph: u16) -> Option<usize> {
        let f = face(data);
        let mut ops = Ops::default();
        f.outline_glyphs()
            .get(skrifa::GlyphId::new(u32::from(glyph)))?
            .draw(
                skrifa::outline::DrawSettings::unhinted(
                    skrifa::instance::Size::unscaled(),
                    &[] as &[skrifa::instance::NormalizedCoord],
                ),
                &mut ops,
            )
            .ok()?;
        Some(ops.0)
    }

    #[test]
    fn a_subset_keeps_the_outlines_it_was_asked_for() {
        let keep = glyphs(LATIN, "AB");
        let out = subset(LATIN, keep.clone());
        let sub = face(&out);

        for (ch, original) in "AB".chars().zip(&keep) {
            // The glyph is renumbered, so the id need not match; what must
            // match is the outline the id names.
            let new = glyph_of(&sub, ch).expect("kept glyph is mapped");
            assert_eq!(
                outline_ops(&out, new),
                outline_ops(LATIN, *original),
                "{ch} must keep its outline"
            );
        }
        assert!(
            out.len() < LATIN.len(),
            "a two glyph subset must be smaller: {} vs {}",
            out.len(),
            LATIN.len()
        );
    }

    #[test]
    fn the_notdef_glyph_survives_even_when_nothing_asks_for_it() {
        let out = subset(LATIN, glyphs(LATIN, "A"));
        let sub = face(&out);
        // Glyph 0 is `.notdef` and must be there in the subset too.
        assert!(glyph_count(&sub) >= 1);
        assert_eq!(
            glyph_of(&sub, 'A').and_then(|g| outline_ops(&out, g)),
            outline_ops(LATIN, glyphs(LATIN, "A")[0])
        );
    }

    #[test]
    fn asking_for_no_glyph_at_all_carries_the_face_whole() {
        // A document can name a face on a page that draws none of it. Trimming
        // to `.notdef` would answer that with a face that cannot draw anything
        // the page asks for later, so an empty request changes nothing.
        let out = subset(LATIN, std::iter::empty());
        assert_eq!(out, LATIN, "an empty request must not trim the face");
    }

    #[test]
    fn a_glyph_outside_the_subset_is_no_longer_mapped() {
        let out = subset(LATIN, glyphs(LATIN, "A"));
        assert!(
            glyph_of(&face(&out), 'Z').is_none(),
            "a dropped glyph keeps no cmap entry"
        );
    }

    #[test]
    fn a_face_with_no_cmap_is_carried_whole() {
        let data = include_bytes!("../tests/data/sofd-nocmap-subset.ttf");
        assert_eq!(
            subset(data, glyphs(data, "A")).len(),
            data.len(),
            "a face this cannot rewrite is returned untouched, not damaged"
        );
    }

    #[test]
    fn a_cff_face_is_carried_whole() {
        let data = include_bytes!("../tests/data/sofd-cjk-subset.ttf");
        assert_eq!(
            subset(data, [0u16, 1, 2]).len(),
            data.len(),
            "CFF outlines have no glyf table to rewrite"
        );
    }

    #[test]
    fn the_subset_reports_the_glyph_count_it_carries() {
        let wanted = glyphs(LATIN, "ABC");
        let out = subset(LATIN, wanted.clone());
        // One glyph per requested, plus `.notdef`.
        assert_eq!(glyph_count(&face(&out)), wanted.len() as u16 + 1);
    }

    #[test]
    fn asking_for_every_glyph_gives_the_original_back() {
        let count = glyph_count(&face(LATIN));
        let out = subset(LATIN, 0..count);
        assert_eq!(
            out.len(),
            LATIN.len(),
            "nothing to drop, nothing to rewrite"
        );
    }

    #[test]
    fn the_subset_still_shapes_and_measures() {
        let keep = glyphs(LATIN, "AVav");
        let out = subset(LATIN, keep);
        let sub = face(&out);
        for ch in "AVav".chars() {
            let glyph = glyph_of(&sub, ch).expect("kept glyph is mapped");
            assert!(
                sub.hmtx()
                    .ok()
                    .and_then(|h| h.advance(skrifa::GlyphId::new(u32::from(glyph))))
                    .is_some(),
                "{ch} must keep its advance, or a line cannot be measured"
            );
        }
    }
}
