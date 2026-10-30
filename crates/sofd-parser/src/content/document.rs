//! Document-level readers: `OFD.xml`, `Document.xml`, pages and templates.

use super::*;
use crate::content::shared::{
    enum_error, read_draw_param_entries, read_element_text, read_layers_strict,
};

/// Parse `Document.xml` into the document model.
pub fn read_document(xml: &str) -> Result<ModelDocument> {
    let mut cursor = Cursor::new(xml);
    let root = cursor.expect_start("Document")?;
    if !root.is("Document") {
        return Err(OfdError::Format(FormatError::NotOfd));
    }
    let mut doc = ModelDocument::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("CommonData") => {
                doc.common_data = read_common_data(&mut cursor)?
            }
            Event::Start(n) if n.is("Pages") => doc.pages = read_pages(&mut cursor)?,
            Event::Start(n) if n.is("VPreferences") => {
                doc.vpreferences = Some(read_vpreferences(&mut cursor)?)
            }
            Event::Start(n) if n.is("Permissions") => {
                doc.permissions = Some(read_permissions(&mut cursor)?)
            }
            // The annotation index, clause 15: a location like any other, so
            // it is read as the text of its own element.
            Event::Start(n) if n.is("Annotations") => {
                doc.annotations = Some(Location::new(trim_leading_slash(&cursor.read_text()?)))
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(doc)
}

/// Read `VPreferences`, clause 7.11. Every child is optional, so what is
/// absent stays absent rather than being filled from the schema's defaults.
fn read_vpreferences(cursor: &mut Cursor<'_>) -> Result<VPreferences> {
    let mut v = VPreferences::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("PageMode") => {
                let text = read_element_text(cursor, &n)?;
                v.page_mode = Some(text.parse().map_err(|_| enum_error(&n, &text))?);
            }
            Event::Start(n) if n.is("PageLayout") => {
                let text = read_element_text(cursor, &n)?;
                v.page_layout = Some(text.parse().map_err(|_| enum_error(&n, &text))?);
            }
            Event::Start(n) if n.is("TabDisplay") => {
                let text = read_element_text(cursor, &n)?;
                v.tab_display = Some(text.parse().map_err(|_| enum_error(&n, &text))?);
            }
            Event::Start(n) if n.is("HideToolbar") => {
                v.hide_toolbar = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("HideMenubar") => {
                v.hide_menubar = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("HideWindowUI") => {
                v.hide_window_ui = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("ZoomMode") => {
                let text = read_element_text(cursor, &n)?;
                let mode = text.parse().map_err(|_| enum_error(&n, &text))?;
                v.zoom = ZoomSetting::ZoomMode(mode);
            }
            Event::Start(n) if n.is("Zoom") => {
                let text = read_element_text(cursor, &n)?;
                let factor = text.parse().map_err(|_| enum_error(&n, &text))?;
                v.zoom = ZoomSetting::Zoom(factor);
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(v)
}

/// An element's text that must read as a boolean.
fn read_bool_element(cursor: &mut Cursor<'_>, node: &Node) -> Result<bool> {
    let text = read_element_text(cursor, node)?;
    parse_bool(node.local(), node.local(), &text)
}

/// Read `Permissions`, clause 7.12. Each flag is independent, so one stated
/// flag does not imply the others; `Print` and `ValidPeriod` are attributes.
fn read_permissions(cursor: &mut Cursor<'_>) -> Result<Permission> {
    let mut p = Permission::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Edit") => p.edit = Some(read_bool_element(cursor, &n)?),
            Event::Start(n) if n.is("Annot") => p.annotate = Some(read_bool_element(cursor, &n)?),
            Event::Start(n) if n.is("Export") => p.export = Some(read_bool_element(cursor, &n)?),
            Event::Start(n) if n.is("Signature") => {
                p.signature = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("Watermark") => {
                p.watermark = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("PrintScreen") => {
                p.print_screen = Some(read_bool_element(cursor, &n)?)
            }
            Event::Start(n) if n.is("Print") => {
                let a = n.attributes();
                p.print = Some(PrintPermission {
                    printable: a.require_as::<bool>("Print", "Printable")?,
                    copies: a.or_default("Print", "Copies", unlimited_copies())?,
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("ValidPeriod") => {
                let a = n.attributes();
                p.valid_period = Some(ValidPeriod {
                    start_date: a.get("StartDate").map(str::to_string),
                    end_date: a.get("EndDate").map(str::to_string),
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(p)
}

fn read_common_data(cursor: &mut Cursor<'_>) -> Result<CommonData> {
    let mut data = CommonData::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("PageArea") => data.page_area = read_page_area(cursor)?,
            // The standard puts `TemplatePage` straight into `CommonData`
            // with no wrapper; only the wrapped form was accepted before.
            Event::Start(n) if n.is("TemplatePage") => {
                let a = n.attributes();
                data.templates
                    .get_or_insert_with(TemplateList::default)
                    .items
                    .push(Template {
                        id: a.require("TemplatePage", "ID")?.to_string(),
                        base_loc: a.require("TemplatePage", "BaseLoc")?.to_string(),
                    });
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("Templates") => data.templates = Some(read_templates(cursor)?),
            Event::Start(n) if n.is("DrawParams") => {
                data.draw_params = Some(read_draw_param_entries(cursor)?);
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(data)
}

fn read_page_area(cursor: &mut Cursor<'_>) -> Result<PageArea> {
    let mut area = PageArea::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("PhysicalBox") => area.physical_box = rect_of(&n, cursor)?,
            Event::Start(n) if n.is("ApplicationBox") => {
                area.application_box = Some(rect_of(&n, cursor)?)
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(area)
}

/// Boxes follow `ST_Box`: four numbers as element text.
/// The attribute form is tolerated for writers that put the box on the element.
fn rect_of(node: &Node, cursor: &mut Cursor<'_>) -> Result<Rect> {
    let a = node.attributes();
    let attr = |names: &[&str]| -> Result<Option<f64>> {
        for name in names {
            if let Some(raw) = a.get(name) {
                return raw.parse().map(Some).map_err(|_| {
                    OfdError::Xml(XmlError::InvalidAttribute {
                        element: node.local().to_string(),
                        attr: (*name).to_string(),
                        value: raw.to_string(),
                    })
                });
            }
        }
        Ok(None)
    };
    let from_attrs = (
        attr(&["Left", "x"])?,
        attr(&["Top", "y"])?,
        attr(&["Width", "w"])?,
        attr(&["Height", "h"])?,
    );
    if let (Some(x), Some(y), Some(w), Some(h)) = from_attrs {
        cursor.skip_element(node)?;
        return Ok(Rect::new(x, y, w, h));
    }
    let raw = cursor.read_text()?;
    let values = parse_number_list_n::<4>(node.local(), "box", &raw)?;
    Ok(Rect::new(values[0], values[1], values[2], values[3]))
}

fn read_templates(cursor: &mut Cursor<'_>) -> Result<TemplateList> {
    let mut list = TemplateList::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Template") || n.is("TemplatePage") => {
                let a = n.attributes();
                list.items.push(Template {
                    id: a.require(n.local(), "ID")?.to_string(),
                    base_loc: a.require(n.local(), "BaseLoc")?.to_string(),
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(list)
}

/// `<FillColor Value="#FF0000" Alpha="255"/>`; `Alpha` is `0..=255`.
/// Clause 8.3.4 also allows the colour to be a shading, written as a child
/// element rather than a `Value` attribute.
pub(crate) fn read_color(node: &Node, cursor: &mut Cursor<'_>) -> Result<ColorRef> {
    let a = node.attributes();
    let value = a.get("Value").unwrap_or("").to_string();
    let alpha = a.optional_as::<u8>("FillColor", "Alpha")?.unwrap_or(255);
    // Clause 7.13: a `Value`'s channels are read in the named colour space,
    // `Index` picking a palette entry. Both travel with the colour so
    // flattening can resolve them against the document's table.
    let space = a.get("ColorSpace").map(str::to_string);
    let index = a.optional_as::<i32>("FillColor", "Index")?;
    let element = node.local();
    let finish = |color: ColorRef| match &space {
        Some(id) => color.with_space(id.clone(), index),
        None => color,
    };
    if node.is_self_closing() {
        return Ok(finish(ColorRef::from_hex(value).with_alpha(alpha)));
    }
    let mut shading = None;
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("AxialShd") => shading = Some(read_axial(&n, cursor)?),
            Event::Start(n) if n.is("RadialShd") => shading = Some(read_radial(&n, cursor)?),
            // Clauses 8.3.4.4 and 8.3.4.5. Both are a coloured mesh; they differ
            // only in how the triangles are written, which `read_gouraud`
            // resolves to one triangle list.
            Event::Start(n) if n.is("GouraudShd") => {
                shading = Some(read_gouraud(&n, cursor, false)?)
            }
            Event::Start(n) if n.is("LaGouraudShd") => {
                shading = Some(read_gouraud(&n, cursor, true)?)
            }
            // Clause 8.3.3: a cell of vector content repeated over the area.
            Event::Start(n) if n.is("Pattern") => shading = Some(read_pattern(&n, cursor)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    let mut color = ColorRef::from_hex(value).with_alpha(alpha);
    color.shading = shading;
    let _ = element;
    Ok(finish(color))
}

/// `AxialShd`: a linear gradient axis with its colour stops.
fn read_axial(node: &Node, cursor: &mut Cursor<'_>) -> Result<Shading> {
    let a = node.attributes();
    let start = parse_position("AxialShd", "StartPoint", a.get("StartPoint"))?;
    let end = parse_position("AxialShd", "EndPoint", a.get("EndPoint"))?;
    let segments = read_segments(cursor)?;
    Ok(Shading::Axial {
        start,
        end,
        segments,
    })
}

/// `RadialShd`: a cone described by two circles and its colour stops.
fn read_radial(node: &Node, cursor: &mut Cursor<'_>) -> Result<Shading> {
    let a = node.attributes();
    let start = parse_position("RadialShd", "StartPoint", a.get("StartPoint"))?;
    let end = parse_position("RadialShd", "EndPoint", a.get("EndPoint"))?;
    let start_radius = a
        .optional_as::<f64>("RadialShd", "StartRadius")?
        .unwrap_or(0.0);
    let end_radius = a
        .optional_as::<f64>("RadialShd", "EndRadius")?
        .unwrap_or(0.0);
    let segments = read_segments(cursor)?;
    Ok(Shading::Radial {
        start,
        start_radius,
        end,
        end_radius,
        segments,
    })
}

/// `GouraudShd` (8.3.4.4) and `LaGouraudShd` (8.3.4.5). Both carry a list of
/// `<Point X Y><Color/></Point>` and a `BackColor`; the lattice form adds a
/// `VerticesPerRow` that says how the points tile into triangles.
fn read_gouraud(node: &Node, cursor: &mut Cursor<'_>, lattice: bool) -> Result<Shading> {
    let per_row = node
        .attributes()
        .optional_as::<usize>("LaGouraudShd", "VerticesPerRow")?;
    let mut points = Vec::new();
    let mut background = None;
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("Point") => points.push(read_mesh_point(&n, cursor)?),
                Event::Start(n) if n.is("BackColor") => {
                    background = Some(Box::new(read_color(&n, cursor)?))
                }
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) | Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    // The strip form of 8.3.4.4 makes one triangle per consecutive triple; the
    // lattice form of 8.3.4.5 makes two per 2x2 cell of `VerticesPerRow`.
    let triangles = if lattice {
        lattice_triangles(points.len(), per_row.unwrap_or(0))
    } else {
        strip_triangles(points.len())
    };
    Ok(Shading::Gouraud {
        points,
        triangles,
        lattice,
        background,
    })
}

/// `Pattern` (8.3.3): a cell of vector content repeated over the filled area.
/// The cell is a page block, read with the same layer reader a page uses, and
/// an unwritten `XStep`/`YStep` defaults to the cell size.
fn read_pattern(node: &Node, cursor: &mut Cursor<'_>) -> Result<Shading> {
    let a = node.attributes();
    let width = a.optional_as::<f64>("Pattern", "Width")?.unwrap_or(0.0);
    let height = a.optional_as::<f64>("Pattern", "Height")?.unwrap_or(0.0);
    let x_step = a.optional_as::<f64>("Pattern", "XStep")?.unwrap_or(0.0);
    let y_step = a.optional_as::<f64>("Pattern", "YStep")?.unwrap_or(0.0);
    let reflect = match a.get("ReflectMethod").unwrap_or("Normal") {
        "Row" => ReflectMethod::Row,
        "Column" => ReflectMethod::Column,
        "RowAndColumn" => ReflectMethod::RowAndColumn,
        _ => ReflectMethod::Normal,
    };
    let relative_to = match a.get("RelativeTo").unwrap_or("Object") {
        "Page" => PatternSpace::Page,
        _ => PatternSpace::Object,
    };
    let ctm = a.get("CTM").map(str::to_string);
    let layers = if node.is_self_closing() {
        Vec::new()
    } else {
        read_pattern_layers(cursor)?
    };
    Ok(Shading::Pattern(Box::new(PatternSpec {
        layers,
        width,
        height,
        x_step,
        y_step,
        reflect,
        relative_to,
        ctm,
    })))
}

/// The `CellContent` block of a `Pattern`, which holds `Layer` children just
/// as a page does.
fn read_pattern_layers(cursor: &mut Cursor<'_>) -> Result<Vec<Layer>> {
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("CellContent") => {
                return if n.is_self_closing() {
                    Ok(Vec::new())
                } else {
                    read_layers_strict(cursor)
                };
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => return Ok(Vec::new()),
            Event::Text(_) => {}
        }
    }
}

/// The strip form of 8.3.4.4: every consecutive group of three vertices is one
/// triangle, so a short list yields as many as it can.
fn strip_triangles(count: usize) -> Vec<MeshTriangle> {
    let mut triangles = Vec::new();
    let mut i = 0;
    while i + 2 < count {
        triangles.push(MeshTriangle {
            a: i,
            b: i + 1,
            c: i + 2,
        });
        i += 3;
    }
    triangles
}

/// The lattice form of 8.3.4.5: `per_row` vertices to a row, each 2x2 cell of the
/// grid making two triangles. A `per_row` below two cannot tile, so it yields
/// nothing rather than a guess.
fn lattice_triangles(count: usize, per_row: usize) -> Vec<MeshTriangle> {
    let mut triangles = Vec::new();
    if per_row < 2 {
        return triangles;
    }
    let rows = count / per_row;
    for row in 0..rows.saturating_sub(1) {
        for col in 0..per_row - 1 {
            let tl = row * per_row + col;
            let tr = tl + 1;
            let bl = tl + per_row;
            let br = bl + 1;
            triangles.push(MeshTriangle {
                a: tl,
                b: tr,
                c: bl,
            });
            triangles.push(MeshTriangle {
                a: tr,
                b: br,
                c: bl,
            });
        }
    }
    triangles
}

/// One `<Point X="0" Y="0"><Color Value="#FF0000"/></Point>` of a mesh.
fn read_mesh_point(node: &Node, cursor: &mut Cursor<'_>) -> Result<MeshPoint> {
    let a = node.attributes();
    let x = a.optional_as::<f64>("Point", "X")?.unwrap_or(0.0);
    let y = a.optional_as::<f64>("Point", "Y")?.unwrap_or(0.0);
    let mut color = ColorRef::default();
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(c) if c.is("Color") => color = read_color(&c, cursor)?,
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) | Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    let alpha = color.clone().alpha();
    Ok(MeshPoint {
        point: (x, y),
        color: color.value,
        alpha,
    })
}

/// `<Segment Position="0.5"><Color Value="#808080"/></Segment>` in sequences of
/// at least two.
fn read_segments(cursor: &mut Cursor<'_>) -> Result<Vec<Segment>> {
    let mut segments = Vec::new();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Segment") => {
                let a = n.attributes();
                let position = a.optional_as::<f64>("Segment", "Position")?.unwrap_or(0.0);
                let mut color = ColorRef::default();
                if n.is_self_closing() {
                    segments.push(Segment { position, color });
                    continue;
                }
                loop {
                    match cursor.next_event()? {
                        Event::Start(c) if c.is("Color") => color = read_color(&c, cursor)?,
                        Event::Start(other) => cursor.skip_element(&other)?,
                        Event::End(_) | Event::Eof => break,
                        Event::Text(_) => {}
                    }
                }
                segments.push(Segment { position, color });
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(segments)
}

/// `StartPoint="0 10"`, a space-separated pair.
fn parse_position(element: &str, attribute: &str, raw: Option<&str>) -> Result<(f64, f64)> {
    let raw = raw.ok_or_else(|| {
        OfdError::Xml(XmlError::MissingAttribute {
            element: element.to_string(),
            attr: attribute.to_string(),
        })
    })?;
    let v = parse_number_list_n::<2>(element, attribute, raw)?;
    Ok((v[0], v[1]))
}

/// `<Clips><Clip><Area><Path .../></Area></Clip></Clips>`. Clause 8.4: the
/// container carries no attribute of its own, an area is one part of a clip,
/// and both a clip and the list of them cover the union of what they name.
fn read_clips(cursor: &mut Cursor<'_>) -> Result<ClipList> {
    let mut list = ClipList::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Clip") => list.items.push(read_clip(cursor)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(list)
}

/// One `Clip`: every `Area` it names, in order.
fn read_clip(cursor: &mut Cursor<'_>) -> Result<Clip> {
    let mut clip = Clip::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Area") => clip.areas.push(read_area(cursor, &n)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(clip)
}

/// One `Area`: its transform, its draw parameter and the path bounding it.
fn read_area(cursor: &mut Cursor<'_>, node: &Node) -> Result<ClipArea> {
    let a = node.attributes();
    let mut area = ClipArea {
        ctm: a.get("CTM").map(str::to_string),
        draw_param: a.get("DrawParam").map(str::to_string),
        ..ClipArea::default()
    };
    if node.is_self_closing() {
        return Ok(area);
    }
    loop {
        match cursor.next_event()? {
            Event::Start(p) if p.is("Path") => {
                area.rule = p.attributes().get("Rule").map(str::to_string);
                area.path_data = read_clip_path(cursor, &p)?;
            }
            Event::Start(t) if t.is("Text") => {
                area.text = Some(read_text_clip(cursor, &t)?);
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(area)
}

/// GB/T 33190 clause 8.4: the text whose glyph outlines bound a clip area.
/// It carries the same run shape as a drawn text object, so the codes and the
/// glyph mapping are read with the same readers.
fn read_text_clip(cursor: &mut Cursor<'_>, node: &Node) -> Result<TextClip> {
    let a = node.attributes();
    let mut clip = TextClip {
        font: a.get("Font").map(str::to_string).unwrap_or_default(),
        size: a.optional_as::<f64>("Text", "Size")?,
        ..TextClip::default()
    };
    if node.is_self_closing() {
        return Ok(clip);
    }
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("TextCode") => {
                let mut code = read_text_code(cursor, &n)?;
                inherit_origin(clip.codes.last_mut(), &mut code);
                clip.codes.push(code);
            }
            Event::Start(n) if n.is("CGTransform") => {
                clip.cg_transforms.push(read_cg_transform(cursor, &n)?)
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(clip)
}

/// The abbreviated data inside an `Area`'s `Path`.
fn read_clip_path(cursor: &mut Cursor<'_>, node: &Node) -> Result<Option<String>> {
    if node.is_self_closing() {
        return Ok(None);
    }
    let mut data = None;
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("AbbreviatedData") => data = Some(cursor.read_text()?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(data)
}

pub(crate) fn read_dash(node: &Node, cursor: &mut Cursor<'_>) -> Result<DashPattern> {
    let a = node.attributes();
    let pattern = match a.get("Value") {
        Some(raw) => parse_number_list::<f64>("DashPattern", "Value", raw)?,
        None => Vec::new(),
    };
    let phase = a
        .optional_as::<f64>("DashPattern", "Offset")?
        .unwrap_or(0.0);
    cursor.skip_element(node)?;
    Ok(DashPattern { phase, pattern })
}

fn read_pages(cursor: &mut Cursor<'_>) -> Result<Pages> {
    let mut pages = Pages::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Page") => pages.pages.push(read_page(cursor, &n)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(pages)
}

fn read_page(cursor: &mut Cursor<'_>, node: &Node) -> Result<Page> {
    let a = node.attributes();
    let mut page = Page {
        id: a.get("ID").unwrap_or("").to_string(),
        base_loc: a.get("BaseLoc").map(str::to_string),
        ..Default::default()
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("Area") => page.area = Some(read_page_area(cursor)?),
                Event::Start(n) if n.is("Content") => {
                    page.content = Some(PageBlock {
                        layers: read_layers_strict(cursor)?,
                    });
                }
                Event::Start(n) if n.is("Template") => {
                    let refs = page.templates.get_or_insert_with(TemplatePageList::default);
                    refs.items.push(TemplateRef {
                        template_id: n
                            .attributes()
                            .require("Template", "TemplateID")?
                            .to_string(),
                        z_order: None,
                    });
                    cursor.skip_element(&n)?;
                }
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(page)
}

/// Parse a standalone page file referenced through `BaseLoc`.
pub fn read_page_file(xml: &str) -> Result<Page> {
    let mut cursor = Cursor::new(xml);
    let root = cursor.expect_start("Page")?;
    let page = read_page(&mut cursor, &root)?;
    Ok(page)
}

/// Parse a template content file into its layers.
/// Template roots vary between writers, so layers are collected wherever they
/// appear under the root instead of assuming one fixed shape.
pub fn read_template_block(xml: &str) -> Result<PageBlock> {
    let mut cursor = Cursor::new(xml);
    let mut layers: Vec<Layer> = Vec::new();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Content") => layers.extend(read_layers_strict(&mut cursor)?),
            // Anything else under the root is stepped over unless it wraps a
            // layer: walking into `Area` is how `PhysicalBox` was taken for an
            // object and the layer beneath it never reached the page.
            Event::Start(n) => {
                if !n.is_self_closing() && !has_layer_under(&cursor, &n)? {
                    cursor.skip_element(&n)?;
                }
            }
            Event::End(_) => break,
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(PageBlock { layers })
}

/// Whether a layer sits at or below an element, so a root that merely wraps the
/// layers is walked into and anything else is stepped over. The cursor is left
/// where it was: this asks a clone of it, not the one being read.
fn has_layer_under(cursor: &Cursor<'_>, node: &Node) -> Result<bool> {
    let mut probe = cursor.clone();
    let mut depth = 1usize;
    while let Ok(event) = probe.next_event() {
        match event {
            Event::Start(n) => {
                if n.is("Layer") {
                    return Ok(true);
                }
                if !n.is_self_closing() {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(false);
                }
            }
            Event::Eof => return Ok(false),
            Event::Text(_) => {}
        }
    }
    let _ = node;
    Ok(false)
}

pub(crate) fn read_layer(cursor: &mut Cursor<'_>, node: &Node) -> Result<Layer> {
    let a = node.attributes();
    let mut layer = Layer {
        id: a.get("ID").map(str::to_string),
        draw_param: a.get("DrawParam").map(str::to_string),
        objects: Vec::new(),
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("PathObject") => {
                    layer
                        .objects
                        .push(Object::PathObject(read_path_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("TextObject") => {
                    layer
                        .objects
                        .push(Object::TextObject(read_text_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("ImageObject") => {
                    layer
                        .objects
                        .push(Object::ImageObject(read_image_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("CompositeObject") => {
                    layer
                        .objects
                        .push(Object::CompositeObject(read_composite_object(cursor, &n)?));
                }
                // Table 3 lets a block hold the same choice a layer does, so a
                // page that groups its objects there loses everything below.
                Event::Start(n) if n.is("PageBlock") => {
                    layer
                        .objects
                        .push(Object::PageBlockObject(read_page_block_object(cursor, &n)?));
                }
                // Vendor extensions are recorded as unknown and skipped at
                // render time instead of failing the whole page.
                Event::Start(other) => {
                    cursor.skip_element(&other)?;
                    layer.objects.push(Object::unknown());
                }
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(layer)
}

fn parse_base(node: &Node) -> Result<ObjectBase> {
    let a = node.attributes();
    let element = node.local();
    let boundary = match a.get("Boundary") {
        Some(raw) => {
            let v = parse_number_list_n::<4>(element, "Boundary", raw)?;
            Rect::new(v[0], v[1], v[2], v[3])
        }
        None => Rect::default(),
    };
    Ok(ObjectBase {
        id: a.get("ID").map(str::to_string),
        boundary,
        ctm: a.get("CTM").map(str::to_string),
        draw_param: a.get("DrawParam").map(str::to_string),
        visible: match a.get("Visible") {
            Some(raw) => Some(parse_bool(element, "Visible", raw)?),
            None => None,
        },
        name: a.get("Name").map(str::to_string),
        // Object opacity, multiplying whatever the fills and strokes say. A
        // producer writes `Alpha="0"` for a backplate it does not want seen;
        // read as opaque, that backplate paints out the page beneath it.
        alpha: a.optional_as::<u8>(element, "Alpha")?,
        // Clause 8.2 fixes these on the graphic unit itself; a draw parameter
        // only supplies them when the object does not.
        line_width: a.optional_as::<f64>(element, "LineWidth")?,
        cap: a.get("Cap").map(|raw| match raw {
            "Round" => LineCapStyle::Round,
            "Square" => LineCapStyle::Square,
            _ => LineCapStyle::Butt,
        }),
        join: a.get("Join").map(|raw| match raw {
            "Round" => LineJoinStyle::Round,
            "Bevel" => LineJoinStyle::Bevel,
            _ => LineJoinStyle::Miter,
        }),
        miter_limit: a.optional_as::<f64>(element, "MiterLimit")?,
        // The paint an object carries lives on the graphic unit, so every kind
        // of object reads it here rather than restating the same four fields.
        fill_color: None,
        stroke_color: None,
        fill: match a.get("Fill") {
            Some(raw) => Some(parse_bool(element, "Fill", raw)?),
            None => None,
        },
        stroke: match a.get("Stroke") {
            Some(raw) => Some(parse_bool(element, "Stroke", raw)?),
            None => None,
        },
        clips: None,
        actions: None,
    })
}

/// GB/T 33190 clause 14: a `PageBlock` groups the same choice of objects a
/// layer holds, and blocks nest, so this reads one level and recurses.
fn read_page_block_object(cursor: &mut Cursor<'_>, node: &Node) -> Result<PageBlockObject> {
    let mut b = PageBlockObject {
        base: parse_base(node)?,
        objects: Vec::new(),
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("PathObject") => {
                    b.objects
                        .push(Object::PathObject(read_path_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("TextObject") => {
                    b.objects
                        .push(Object::TextObject(read_text_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("ImageObject") => {
                    b.objects
                        .push(Object::ImageObject(read_image_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("CompositeObject") => {
                    b.objects
                        .push(Object::CompositeObject(read_composite_object(cursor, &n)?));
                }
                Event::Start(n) if n.is("PageBlock") => {
                    b.objects
                        .push(Object::PageBlockObject(read_page_block_object(cursor, &n)?));
                }
                Event::Start(other) => {
                    cursor.skip_element(&other)?;
                    b.objects.push(Object::unknown());
                }
                Event::End(_) | Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(b)
}

/// GB/T 33190 clause 13: a composite object places a vector resource on the
/// page, so it only carries a resource reference on top of the object base.
fn read_composite_object(cursor: &mut Cursor<'_>, node: &Node) -> Result<CompositeObject> {
    let mut c = CompositeObject {
        base: parse_base(node)?,
        // `CT_Composite` names its resource on the element itself.
        resource_id: node
            .attributes()
            .get("ResourceID")
            .unwrap_or_default()
            .to_string(),
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("Actions") => c.base.actions = Some(read_actions(cursor)?),
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) | Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(c)
}

/// The `Actions` element a graphic unit may carry, clause 14.1.
fn read_actions(cursor: &mut Cursor<'_>) -> Result<Actions> {
    let mut actions = Actions::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Action") => actions.items.push(read_action(cursor, &n)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(actions)
}

/// One `Action`: the event that fires it, an optional region, and the thing it
/// does. `Event` is required by the schema, so an unknown spelling is an error
/// rather than a silent default.
fn read_action(cursor: &mut Cursor<'_>, node: &Node) -> Result<Action> {
    let raw = node.attributes().require("Action", "Event")?;
    let event = ActionEvent::parse(raw).ok_or_else(|| {
        OfdError::Xml(XmlError::InvalidAttribute {
            element: "Action".to_string(),
            attr: "Event".to_string(),
            value: raw.to_string(),
        })
    })?;
    let mut action = Action {
        event,
        region: None,
        kind: ActionKind::Unknown,
    };
    if node.is_self_closing() {
        return Ok(action);
    }
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Region") => action.region = Some(read_region(cursor)?),
            Event::Start(n) if n.is("Goto") => action.kind = read_goto(cursor)?,
            Event::Start(n) if n.is("URI") => {
                let a = n.attributes();
                action.kind = ActionKind::Uri(UriAction {
                    uri: a.get("URI").unwrap_or_default().to_string(),
                    base: a.get("Base").map(str::to_string),
                    target: a.get("Target").map(str::to_string),
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("GotoA") => {
                let a = n.attributes();
                action.kind = ActionKind::GotoA {
                    attach_id: a.get("AttachID").unwrap_or_default().to_string(),
                    new_window: match a.get("NewWindow") {
                        Some(raw) => parse_bool("GotoA", "NewWindow", raw)?,
                        None => true,
                    },
                };
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("Sound") => {
                let a = n.attributes();
                action.kind = ActionKind::Sound(SoundAction {
                    resource_id: a.get("ResourceID").unwrap_or_default().to_string(),
                    volume: a.optional_as::<i32>("Sound", "Volume")?,
                    repeat: match a.get("Repeat") {
                        Some(raw) => Some(parse_bool("Sound", "Repeat", raw)?),
                        None => None,
                    },
                    synchronous: match a.get("Synchronous") {
                        Some(raw) => Some(parse_bool("Sound", "Synchronous", raw)?),
                        None => None,
                    },
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("Movie") => {
                let a = n.attributes();
                action.kind = ActionKind::Movie(MovieAction {
                    resource_id: a.get("ResourceID").unwrap_or_default().to_string(),
                    operator: MovieOperator::parse(a.get("Operator").unwrap_or("Play")),
                });
                cursor.skip_element(&n)?;
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(action)
}

/// The region an action is sensitive in, `CT_Region`.
fn read_region(cursor: &mut Cursor<'_>) -> Result<Region> {
    let mut region = Region::default();
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Area") => region.areas.push(read_region_area(cursor, &n)?),
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(region)
}

/// One `Area` of a region, flattened into a path so a hit test can use it.
fn read_region_area(cursor: &mut Cursor<'_>, node: &Node) -> Result<RegionArea> {
    let mut area = RegionArea::default();
    if node.is_self_closing() {
        return Ok(area);
    }
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Move") => {
                area.path_data.push_str(&command("M", &n, &["Point1"])?);
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("Line") => {
                area.path_data.push_str(&command("L", &n, &["Point1"])?);
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("QuadraticBezier") => {
                area.path_data
                    .push_str(&command("Q", &n, &["Point1", "Point2"])?);
                cursor.skip_element(&n)?;
            }
            Event::Start(n) if n.is("CubicBezier") => {
                area.path_data
                    .push_str(&command("C", &n, &["Point1", "Point2", "Point3"])?);
                cursor.skip_element(&n)?;
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(area)
}

/// Turn one region command into its abbreviated form, `M x y`, `L x y`, and so on.
fn command(letter: &str, node: &Node, points: &[&str]) -> Result<String> {
    let mut out = String::from(letter);
    for name in points {
        let raw = node.attributes().get(name).unwrap_or("0 0");
        let v = sofd_xml::parse_number_list::<f64>(node.local(), name, raw)?;
        for n in v {
            out.push(' ');
            out.push_str(&n.to_string());
        }
    }
    out.push(' ');
    Ok(out)
}

/// What a `Goto` jumps to: a destination on a page, or a bookmark.
fn read_goto(cursor: &mut Cursor<'_>) -> Result<ActionKind> {
    let mut target = None;
    loop {
        match cursor.next_event()? {
            Event::Start(n) if n.is("Dest") => {
                let a = n.attributes();
                target = Some(GotoTarget::Dest(Dest {
                    kind: a.get("Type").unwrap_or_default().to_string(),
                    page_id: a.get("PageID").unwrap_or_default().to_string(),
                    left: a.get("Left").map(str::to_string),
                    top: a.get("Top").map(str::to_string),
                    right: a.get("Right").map(str::to_string),
                    bottom: a.get("Bottom").map(str::to_string),
                    zoom: a.get("Zoom").map(str::to_string),
                }));
            }
            Event::Start(n) if n.is("Bookmark") => {
                target = Some(GotoTarget::Bookmark {
                    name: n.attributes().get("Name").unwrap_or_default().to_string(),
                });
            }
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) | Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(match target {
        Some(t) => ActionKind::Goto(t),
        None => ActionKind::Unknown,
    })
}

fn read_path_object(cursor: &mut Cursor<'_>, node: &Node) -> Result<PathObject> {
    let a = node.attributes();
    let mut p = PathObject {
        base: parse_base(node)?,
        rule: match a.get("Rule") {
            Some("Even-Odd") => Some(FillRuleStyle::EvenOdd),
            Some(_) => Some(FillRuleStyle::NonZero),
            None => None,
        },
        ..Default::default()
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("AbbreviatedData") => {
                    p.abbreviated_data = cursor.read_text()?
                }
                Event::Start(n) if n.is("FillColor") => {
                    p.base.fill_color = Some(read_color(&n, cursor)?)
                }
                Event::Start(n) if n.is("StrokeColor") => {
                    p.base.stroke_color = Some(read_color(&n, cursor)?)
                }
                Event::Start(n) if n.is("Clips") => p.base.clips = Some(read_clips(cursor)?),
                Event::Start(n) if n.is("Actions") => p.base.actions = Some(read_actions(cursor)?),
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(p)
}

fn read_text_object(cursor: &mut Cursor<'_>, node: &Node) -> Result<TextObject> {
    let a = node.attributes();
    let mut t = TextObject {
        base: parse_base(node)?,
        font: a.get("Font").unwrap_or("").to_string(),
        size: Millimeter::new(a.optional_as::<f64>("TextObject", "Size")?.unwrap_or(3.0)),
        char_space: a.optional_as::<f64>("TextObject", "CharSpace")?,
        h_scale: a.optional_as::<f64>("TextObject", "HScale")?,
        read_direction: angle_of(a.get("ReadDirection")).map(ReadDirection),
        char_direction: angle_of(a.get("CharDirection")).map(CharDirection),
        ..Default::default()
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("TextCode") => {
                    let mut code = read_text_code(cursor, &n)?;
                    inherit_origin(t.codes.last_mut(), &mut code);
                    t.codes.push(code);
                }
                Event::Start(n) if n.is("CGTransform") => {
                    t.cg_transforms.push(read_cg_transform(cursor, &n)?)
                }
                Event::Start(n) if n.is("StrokeColor") => {
                    t.base.stroke_color = Some(read_color(&n, cursor)?)
                }
                Event::Start(n) if n.is("FillColor") => {
                    t.base.fill_color = Some(read_color(&n, cursor)?)
                }
                Event::Start(n) if n.is("Clips") => t.base.clips = Some(read_clips(cursor)?),
                Event::Start(n) if n.is("Actions") => t.base.actions = Some(read_actions(cursor)?),
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(t)
}

/// GB/T 33190 clause 11.4: a `CGTransform` covers `CodeCount` characters from
/// `CodePosition` and names the glyphs that replace them; both default to one.
fn read_cg_transform(cursor: &mut Cursor<'_>, node: &Node) -> Result<CGTransform> {
    let a = node.attributes();
    let mut t = CGTransform {
        code_position: a
            .optional_as::<usize>("CGTransform", "CodePosition")?
            .unwrap_or(0),
        code_count: a
            .optional_as::<usize>("CGTransform", "CodeCount")?
            .unwrap_or(1),
        glyph_count: a
            .optional_as::<usize>("CGTransform", "GlyphCount")?
            .unwrap_or(1),
        glyphs: Vec::new(),
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("Glyphs") => {
                    let raw = cursor.read_text()?;
                    t.glyphs = parse_number_list("CGTransform", "Glyphs", &raw)?;
                }
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(t)
}

/// Parse a `DeltaX`/`DeltaY` list: bare numbers, or `g count value`, which
/// repeats `value` for `count` offsets. Both forms are in the wild; how the
/// list is then read against the text is `flatten::GlyphAt`'s question.
fn parse_delta_list(element: &str, attr: &str, raw: &str) -> Result<Vec<f64>> {
    let mut out: Vec<f64> = Vec::new();
    let mut tokens = raw.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("g") {
            let count = match tokens.next() {
                Some(n) => n.parse::<usize>().map_err(|_| invalid(element, attr, n))?,
                None => return Err(invalid(element, attr, raw)),
            };
            let value = match tokens.next() {
                Some(v) => v.parse::<f64>().map_err(|_| invalid(element, attr, v))?,
                None => return Err(invalid(element, attr, raw)),
            };
            out.extend(std::iter::repeat_n(value, count));
            continue;
        }
        out.push(
            token
                .parse::<f64>()
                .map_err(|_| invalid(element, attr, raw))?,
        );
    }
    Ok(out)
}

fn invalid(element: &str, attr: &str, value: &str) -> OfdError {
    OfdError::Xml(sofd_error::XmlError::InvalidAttribute {
        element: element.to_string(),
        attr: attr.to_string(),
        value: value.to_string(),
    })
}

fn read_text_code(cursor: &mut Cursor<'_>, node: &Node) -> Result<TextCode> {
    let a = node.attributes();
    let x = a.optional_as::<f64>("TextCode", "X")?;
    let y = a.optional_as::<f64>("TextCode", "Y")?;
    let mut code = TextCode::default();
    code.x = x.unwrap_or(0.0);
    code.y = y.unwrap_or(0.0);
    code.delta_x = match a.get("DeltaX") {
        Some(raw) => parse_delta_list("TextCode", "DeltaX", raw)?,
        None => Vec::new(),
    };
    code.delta_y = match a.get("DeltaY") {
        Some(raw) => parse_delta_list("TextCode", "DeltaY", raw)?,
        None => Vec::new(),
    };
    code.text = if node.is_self_closing() {
        String::new()
    } else {
        cursor.read_text()?
    };
    Ok(code)
}

/// Table 47: an angle turned clockwise from `+x`. A value the table does not
/// name, or one that is not a number, keeps the default rather than failing
/// the open: the attribute states a presentation, not a structure.
fn angle_of(raw: Option<&str>) -> Option<u16> {
    let raw = raw?;
    let Ok(degrees) = raw.parse::<i64>() else {
        return Some(0);
    };
    Some(match degrees.rem_euclid(360) {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    })
}

/// Table 46: a `TextCode` that states no `X` or `Y` takes the last one the
/// object stated, so a run of codes is one line at one origin.
fn inherit_origin(previous: Option<&mut TextCode>, code: &mut TextCode) {
    let Some(previous) = previous else { return };
    previous.carry_origin_to();
    let (x, y) = previous.origin();
    code.x = x;
    code.y = y;
}

fn read_image_object(cursor: &mut Cursor<'_>, node: &Node) -> Result<ImageObject> {
    let a = node.attributes();
    let mut i = ImageObject {
        base: parse_base(node)?,
        resource_id: a.get("ResourceID").unwrap_or("").to_string(),
        ..Default::default()
    };
    if !node.is_self_closing() {
        loop {
            match cursor.next_event()? {
                Event::Start(n) if n.is("Substitution") => {
                    i.substitution = Some(read_color(&n, cursor)?)
                }
                Event::Start(n) if n.is("Clips") => i.base.clips = Some(read_clips(cursor)?),
                Event::Start(n) if n.is("Actions") => i.base.actions = Some(read_actions(cursor)?),
                Event::Start(other) => cursor.skip_element(&other)?,
                Event::End(_) => break,
                Event::Eof => break,
                Event::Text(_) => {}
            }
        }
    }
    Ok(i)
}
