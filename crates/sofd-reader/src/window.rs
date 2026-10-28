//! The window's zones, drawn against a `Ui` so they can run headlessly.
//! Everything here obeys `docs/Ui-spec.md`: panels in a fixed order, the canvas
//! state asked of `App`, and no string drawn that the catalogue did not give.

use crate::message::ReaderMessage;
use eframe::egui::{self, TextureOptions, Ui};
use sofd_i18n::MessageId;

use crate::app::{App, Canvas, Fit, TextSize, ThemeChoice, Verdict};
use crate::export_worker::ExportFormat;
use crate::icons::{self, Icon};
use crate::shortcuts;
use crate::theme::{self, Style, Token};
use crate::viewer::{DroppedFile, Layout, MAX_ZOOM, MIN_ZOOM, POINTS_PER_MM};

/// The zoom range the slider offers, in percent.
const MIN_PERCENT: f32 = MIN_ZOOM * 100.0;
const MAX_PERCENT: f32 = MAX_ZOOM * 100.0;

/// The width the zoom slider takes, in points: the handle's travel, the step
/// between the track and the percent box, and the box itself -- measured on a
/// drawn slider, 100 plus 6 plus 47.7. A slider is the one control in either
/// bar that cannot shrink to an icon, so on a strip narrower than this the
/// toolbar drops its buttons to leave it room, and below that it goes rather
/// than being drawn off the edge, with the status bar still naming the page.
const SLIDER_ROOM: f32 = 154.0;

/// How many controls the export group has: print, PDF, PNG. They travel
/// together — two formats without the print button read as a group that never
/// had it — so the count is one decision rather than three.
const EXPORTS: usize = 3;

/// The widest the zoom track is drawn: the size a bar with room to spare gives
/// it, so the control reads the same on a tablet as on a phone.
const MAX_TRACK: f32 = 100.0;

/// The narrowest track worth drawing: below this the handle has nowhere to go,
/// and the bar is better off folding the control away than drawing a nub.
const MIN_TRACK: f32 = 40.0;

/// What the zoom control costs besides its track, as drawn: the step between
/// the track and the value box, and the box itself, measured at 16 plus 41.
/// `egui` sizes that box from `interact_size.x` less two button paddings and
/// then pads it again to draw, so the box comes out wider than the row it was
/// charged for, and the extra lands on the bar's right-hand edge.
const SLIDER_CHROME: f32 = 57.0;

/// The extra the value box draws beyond the room `egui` gives it: twice the
/// button padding less the four points its text keeps. Read from the style for
/// the same reason every other measure is.
fn value_box_overhang(ui: &Ui) -> f32 {
    (2.0 * ui.spacing().button_padding.x - 4.0).max(0.0)
}

/// The hairline an atom's picture costs beside its stated size, as drawn: a
/// 20-point glyph in a button measures 46 points wide, not 44. Kept as its own
/// number so the arithmetic has one place to change when `egui` moves.
const ICON_HAIRLINE: f32 = 2.0;

/// What the window wants the host to do after a frame, if anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowRequests {
    /// The reader asked for a file; the host opens the platform dialog.
    pub open_dialog: bool,
    /// The reader asked where to write an export; the host asks the platform.
    pub export: Option<ExportFormat>,
    /// The reader asked to print; the host lays the job out on the platform's
    /// paper and hands the sheets to a printer.
    pub print: bool,
    /// The reader asked to pick a second revision to compare against.
    pub compare: bool,
    /// The reader wants the window closed.
    pub quit: bool,
}

/// Whether the reader can be asked for a document at all. Both platforms can:
/// a desktop through `rfd`, a device through the system's own picker, which
/// answers on the activity that asked for it.
const CAN_ASK_FOR_A_DOCUMENT: bool = true;

/// Whether the reader has a dialog of its own to reach. Android has none, and
/// the second revision a comparison reads would have to come from one, so that
/// action is left out rather than drawn as a button that does nothing.
#[cfg(not(target_os = "android"))]
const HAS_FILE_DIALOG: bool = true;

/// The same, on the one system with no dialog to reach.
#[cfg(target_os = "android")]
const HAS_FILE_DIALOG: bool = false;

/// Draw one frame into `ui`, and report what the reader asked for.
///
/// The specification's steps in order; nothing here reaches a platform.
pub fn draw(ui: &mut Ui, app: &mut App) -> WindowRequests {
    let mut requests = WindowRequests::default();
    // Inset before a single zone is declared: the zones stack from the window's
    // corner, and a bar laid out from an origin the reader cannot see is a bar
    // with its controls under the clock.
    let safe = safe_area(ui);
    let mut frame = ui.new_child(egui::UiBuilder::new().max_rect(inset(ui.max_rect(), safe)));
    let ui = &mut frame;
    // Step 1 collects finished rasters before anything is drawn, so a page the
    // worker finished during the last frame is on screen in this one.
    app.poll();
    // Appearance lands before the first widget is built: `egui` resolves a
    // style when a widget is made, so a late push would flash for one frame.
    apply_appearance(ui, app);
    // A document the platform handed over before this frame counts as an open
    // request that has already been answered, so it lands before the panels
    // read the document they would otherwise describe as absent.
    app.take_handed_over();
    take_drops(ui, app);
    // A panel reads the style snapshot taken before `draw` ran, so a frame that
    // does not hand this over draws with `egui`'s own spacing and measures the
    // bars as if they had room they do not have.
    let style = ui.ctx().style_of(ui.ctx().theme());
    draw_menu(ui, app, &mut requests, &style);
    draw_toolbar(ui, app, &style);
    draw_search(ui, app, &style);
    draw_signatures(ui, app, &style);
    draw_annotations(ui, app, &style);
    draw_comparison(ui, app, &mut requests, &style);
    draw_canvas(ui, app, &style);
    draw_status(ui, app, &style);
    // Shortcuts resolve after every panel has been drawn, because a key the
    // search box already took must not also turn the page underneath it.
    take_shortcuts(ui, app, &mut requests);
    if app.wants_quit() {
        requests.quit = true;
    }
    // The request is not consumed here: the host consumes it, because the host
    // is what asks the platform where a file should land, and it needs the
    // page selection this window does not own.
    requests.export = app.wants_export();
    requests.print = app.wants_print();
    requests
}

/// The room the platform's own bars take, which the window must not draw under.
///
/// A device that hides its status bar draws it translucent over the whole
/// screen, so a header laid out from the window's corner sits under the clock.
/// The gap between content rectangle and window is what to keep clear; zero
/// until something states it, as on a desktop or a phone's first frame.
pub fn safe_area(ui: &Ui) -> egui::epaint::MarginF32 {
    ui.ctx()
        .data(|data| data.get_temp::<egui::epaint::MarginF32>(safe_area_id()))
        .unwrap_or(egui::epaint::MarginF32::ZERO)
}

/// State the room the platform's bars take.
///
/// Called by whoever owns a window, which is the only place a platform answers.
pub fn set_safe_area(ctx: &egui::Context, safe: egui::epaint::MarginF32) {
    ctx.data_mut(|data| data.insert_temp(safe_area_id(), safe));
}

/// The key the window's content rectangle is remembered under.
fn safe_area_id() -> egui::Id {
    egui::Id::new("reader-safe-area")
}

/// The room the bars take, from a window measured in physical pixels.
///
/// Divided by the scale the window is drawn at, so a phone at 3x does not keep
/// three status bars clear. Each edge is measured against the window it is cut
/// from, since the rectangle excludes the bars and what is wanted is how much
/// each one covers.
#[cfg(target_os = "android")]
pub fn measured_insets(
    width: i32,
    height: i32,
    content: &android_activity::Rect,
    scale: f32,
) -> egui::epaint::MarginF32 {
    let points = |pixels: i32| pixels.max(0) as f32 / scale.max(f32::EPSILON);
    egui::epaint::MarginF32 {
        left: points(content.left),
        top: points(content.top),
        right: points(width - content.right),
        bottom: points(height - content.bottom),
    }
}

/// Read the activity's content rectangle and state it as the window's inset.
#[cfg(target_os = "android")]
pub fn remember_safe_area(
    ctx: &egui::Context,
    window: &winit::window::Window,
    scale: f32,
) -> egui::epaint::MarginF32 {
    use winit::platform::android::WindowExtAndroid;
    let content = window.content_rect();
    let full = window.inner_size();
    let safe = measured_insets(full.width as i32, full.height as i32, &content, scale);
    set_safe_area(ctx, safe);
    safe
}

/// The window with `safe` taken off each edge, and never less than nothing.
///
/// A margin larger than the window would make a rectangle of negative size,
/// which `egui` asserts on; a device that reported one is a device whose bars
/// have left no room for the reader at all, so the window is what it has.
pub fn inset(rect: egui::Rect, safe: egui::epaint::MarginF32) -> egui::Rect {
    let left = safe.left.max(0.0).min(rect.width());
    let right = safe.right.max(0.0).min(rect.width() - left);
    let top = safe.top.max(0.0).min(rect.height());
    let bottom = safe.bottom.max(0.0).min(rect.height() - top);
    egui::Rect::from_min_max(
        rect.min + egui::vec2(left, top),
        rect.max - egui::vec2(right, bottom),
    )
}

/// Step 3: push the reader's theme, text size and contrast into `egui`. The
/// setting belongs to `App`; this is the one place it reaches the toolkit.
fn apply_appearance(ui: &mut Ui, app: &App) {
    let theme = match app.theme() {
        ThemeChoice::System => ui.ctx().system_theme().unwrap_or(egui::Theme::Light),
        ThemeChoice::Light => egui::Theme::Light,
        ThemeChoice::Dark => egui::Theme::Dark,
    };
    let scale = app.text_size().scale();
    let sharp = app.high_contrast();
    // One scheme is dressed per pass, and only when a setting moved: rebuilding
    // both style tables every frame would re-rasterize the glyph atlas.
    let stamp = (theme, app.text_size(), sharp);
    let applied = ui
        .ctx()
        .data(|data| data.get_temp::<(egui::Theme, TextSize, bool)>(appearance_id()));
    if applied != Some(stamp) {
        theme::apply(ui.ctx(), theme, scale, sharp);
        // Dressing fills that scheme's table; which table gets read is the
        // active theme. Without this, picking Light drew from the dark one.
        ui.ctx().set_theme(theme);
        ui.ctx()
            .data_mut(|data| data.insert_temp(appearance_id(), stamp));
    }
}

/// The key the applied appearance is remembered under, so it is rebuilt once.
fn appearance_id() -> egui::Id {
    egui::Id::new("reader-appearance")
}

/// One of the window's colours, in the scheme and contrast in force.
fn colour(ui: &Ui, app: &App, token: Token) -> egui::Color32 {
    theme::colour(ui.ctx(), app.high_contrast(), token)
}

/// A label in one of the window's colours and one of its named styles. The
/// point size is set here rather than looked up in the style table, because a
/// popup is built before step 3 has registered the named styles in it.
fn toned(
    ui: &Ui,
    app: &App,
    text: impl Into<String>,
    token: Token,
    style: Style,
) -> egui::RichText {
    let scale = app.text_size().scale();
    egui::RichText::new(text)
        .color(colour(ui, app, token))
        .size(style.points(scale))
}

/// Resolve the keys pressed this frame into intent, after the panels have
/// declined them, so a query being typed does not page the document. Escape
/// is read apart: leaving the search box and closing the panel share a key.
fn take_shortcuts(ui: &mut Ui, app: &mut App, requests: &mut WindowRequests) {
    let ctx = ui.ctx().clone();
    let focused = ctx.memory(|memory| memory.has_focus(shortcuts::search_box_id()));
    let mut escape = None;
    for key in pressed_keys(&ctx) {
        let modifiers = ctx.input(|input| input.modifiers);
        if key == egui::Key::Escape {
            escape = shortcuts::escape_target(app.search_open(), focused);
            continue;
        }
        let Some(action) = shortcuts::interpret(key, modifiers, app.is_open()) else {
            continue;
        };
        if action == shortcuts::Action::FocusSearch {
            ctx.memory_mut(|memory| memory.request_focus(shortcuts::search_box_id()));
            if !app.search_open() {
                app.set_search_open(true);
            }
            continue;
        }
        if action == shortcuts::Action::Escape {
            continue;
        }
        act(app, action, requests);
    }
    // Escape no panel claimed leaves the search box, because a caret the
    // reader cannot get out of is this window's one real trap.
    if let Some(action) = escape {
        match action {
            shortcuts::Action::FocusSearch => {
                ctx.memory_mut(|memory| memory.surrender_focus(shortcuts::search_box_id()));
            }
            shortcuts::Action::ToggleSearch => app.set_search_open(false),
            shortcuts::Action::Escape => {}
            _ => {}
        }
    }
}

/// Apply one resolved action, routing the ones that need the host.
fn act(app: &mut App, action: shortcuts::Action, requests: &mut WindowRequests) {
    if action == shortcuts::Action::Open {
        app.apply(action);
        requests.open_dialog = true;
        return;
    }
    if action == shortcuts::Action::ExportPdf {
        app.apply(action);
        requests.export = app.wants_export();
        return;
    }
    if action == shortcuts::Action::Print {
        app.apply(action);
        requests.print = app.wants_print();
        return;
    }
    app.apply(action);
}

/// Every key pressed this frame, in the order pressed.
fn pressed_keys(ctx: &egui::Context) -> Vec<egui::Key> {
    ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    ..
                } => Some(*key),
                _ => None,
            })
            .collect()
    })
}

/// Consume the open request and anything dropped on the window this frame.
fn take_drops(ui: &mut Ui, app: &mut App) {
    let dropped: Vec<egui::DroppedFile> = ui.input(|i| i.raw.dropped_files.clone());
    for file in dropped {
        if let Err(err) = app.open_dropped(to_dropped(&file)) {
            app.set_error(err.message);
        }
    }
}

/// Z1: the header bar — document actions at the start, the view switcher in the
/// centre, the main menu at the end. A control is placed by what it does, and
/// state that is only read lives in Z4, so the bar stays narrow enough to drag.
fn draw_menu(ui: &mut Ui, app: &mut App, requests: &mut WindowRequests, style: &egui::Style) {
    egui::Panel::top("menu").show(ui, |ui| {
        ui.set_style(style.clone());
        let gap = theme::Space::Small.points();
        let groups = theme::Space::Medium.points();
        // Priced first, since the menu sits on the bar's right margin and the
        // switcher gets what is left. Pricing only the controls drew the menu
        // past the right edge on a 320dp phone.
        let reserved = menu_side();
        ui.horizontal(|ui| {
            // The row starts two margins narrower than the bar, so the account
            // is kept in the row's own coordinates.
            let mut left = ui.available_width()
                - theme::Space::Small.points()
                - plate_margin()
                - groups
                - groups
                - reserved;
            // The switcher is all three or none: two halves of a three-way
            // choice is a reader who cannot reach the view they wanted.
            let switched = fits_into(&mut left, run(ui, 3) + plate_margin(), 3);
            // The clip is opened out to the window, as in the toolbar: the row's
            // clip is the panel's, two margins narrower than the bar itself.
            let mut clip = ui.clip_rect();
            clip.max.x += panel_inset(ui) / 2.0;
            ui.set_clip_rect(clip);
            // The plate the two document actions sit on has its own inner
            // margin, which is the same small step on each side.
            ui.add_space(gap);
            pair(ui, |ui| draw_document_actions(ui, app, requests));
            ui.add_space(groups);
            group(ui, switched, |ui| draw_view_switcher(ui, app, switched));
            // By the bar's right margin, not the blank left over: the row's
            // cursor stopped two margins short, and the leftover blank pushed
            // the menu past the edge on a 320dp phone.
            let mut end = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(ui.max_rect())
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            end.add_space(gap);
            draw_main_menu(&mut end, app, switched);
        });
    });
}

/// How many of a group's controls fit in `left`, given what the group costs as
/// it is drawn -- its run, its plate's margins -- and the step after it.
///
/// A group is all or nothing: half a paging group is two arrows and no page.
/// Not drawing it whole is also what keeps the arithmetic honest, since a blank
/// cannot come out of a group charged for controls it never drew.
fn fits_into(left: &mut f32, cost: f32, count: usize) -> usize {
    let price = cost + theme::Space::Medium.points();
    if *left < price {
        0
    } else {
        *left -= price;
        count
    }
}

/// What `count` controls cost before the group step after them: the run as it
/// is drawn inside one group.
fn run(ui: &Ui, count: usize) -> f32 {
    if count == 0 {
        0.0
    } else {
        count as f32 * icon_side(ui) + (count - 1) as f32 * theme::ITEM_GAP * ui.ctx().zoom_factor()
    }
}

/// The points a panel's frame spends on either side of its row.
///
/// `Panel::top` keeps an inner margin of its own, already taken out of the
/// row's `available_width`, so a run measured against the row sits in a frame
/// narrower than the window. Read from the style rather than written down, so a
/// theme that changes it moves the account with it.
fn panel_inset(ui: &Ui) -> f32 {
    let frame = egui::Frame::side_top_panel(ui.style());
    2.0 * frame.inner_margin.sum().x.max(0.0)
}

/// The width one icon button takes on the bar.
///
/// What `tests/window.rs` measures a drawn button at, not a prediction: the
/// glyph, the button's padding twice, and the hairline `egui` adds round a
/// picture. Counting the glyph alone under-reported every group by more than
/// half, which is how the zoom came to be drawn on top of the paging.
fn icon_side(ui: &Ui) -> f32 {
    let glyph = icons::box_size(1.0).x;
    glyph + 2.0 * ui.spacing().button_padding.x + ICON_HAIRLINE
}

/// The width the main menu's button takes on the bar.
///
/// The same drawing as an icon button less what it does not do: no frame, its
/// own box, so no padding and no text button's minimum height. Pricing it as one
/// left the bar twelve points short, and a 320dp header folded a switcher it had
/// room to keep.
fn menu_side() -> f32 {
    icons::box_size(1.0).x + ICON_HAIRLINE
}

/// What a group's plate spends on its own left and right edges: one small step
/// each. A group therefore costs this much more than its run, wherever it sits.
fn plate_margin() -> f32 {
    2.0 * theme::Space::Small.points()
}

/// Draw a run of controls whose glyphs belong together: a linked group drawn as
/// one rounded plate behind them, a step of space inside so the glyphs clear
/// the plate's edge. A group of nothing has no plate: one that still drew it
/// showed an empty lozenge at the start of a phone's toolbar.
fn pair(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    if ui.available_width() <= 0.0 {
        return;
    }
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(theme::CORNER_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            theme::Space::Small.points() as i8,
            0,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::ITEM_GAP;
                contents(ui);
            });
        });
}

/// Draw a group on a plate, or nothing at all when it has no controls to show.
/// The pair are one decision: a group either has members and is plated, or is
/// absent, and the bar's account prices it the same way.
fn group(ui: &mut Ui, shown: usize, contents: impl FnOnce(&mut Ui)) {
    if shown == 0 {
        return;
    }
    pair(ui, contents);
}

/// An icon button: a header control with no frame, no background and no label.
/// The glyph is the whole control, which is the form the guideline gives a
/// header button, and its tooltip is the word it would otherwise show.
fn icon_button(ui: &mut Ui, enabled: bool, sharp: bool, glyph: Glyph<'_>) -> egui::Response {
    glyph.button(ui, enabled, sharp, false)
}

/// An icon toggle in a view switcher: the same glyph, on a rounded plate while
/// it is the one showing, which is the shape the pattern gives a chosen view.
fn icon_toggle(
    ui: &mut Ui,
    enabled: bool,
    sharp: bool,
    on: bool,
    glyph: Glyph<'_>,
) -> egui::Response {
    glyph.button(ui, enabled, sharp, on)
}

/// A glyph and the words it stands for. The words are the tooltip and the
/// accessible name, because a drawing has neither and a control needs both.
struct Glyph<'a> {
    icon: Icon,
    hint: &'a str,
}

impl Glyph<'_> {
    /// A real `Button` whose picture is the glyph, so the toolkit draws the
    /// hover plate and answers the accessibility tree. The picture is a texture
    /// because a button lays out its contents before it knows where they land.
    fn button(self, ui: &mut Ui, enabled: bool, sharp: bool, on: bool) -> egui::Response {
        let ink = if enabled {
            theme::colour(ui.ctx(), sharp, Token::Text)
        } else {
            ui.visuals().weak_text_color()
        };
        let side = icons::box_size(1.0);
        let handle = icons::texture(ui.ctx(), self.icon, ink);
        let image = egui::Image::from_texture(egui::load::SizedTexture::new(handle.id(), side))
            .fit_to_exact_size(side);
        let mut button = egui::Button::image_and_text(image, "")
            .frame_when_inactive(on)
            .min_size(side);
        if on {
            button = button.selected(true);
        }
        let response = ui.add_enabled(enabled, button).on_hover_text(self.hint);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, self.hint)
        });
        if enabled {
            response
        } else {
            response.on_disabled_hover_text(self.hint)
        }
    }
}

/// The two actions that change which document is open. Both carry a tooltip,
/// because the glyph alone has to be recognised: "close" on a document is not
/// obviously "close this one".
fn draw_document_actions(ui: &mut Ui, app: &mut App, requests: &mut WindowRequests) {
    let sharp = app.high_contrast();
    let open = app.text(ReaderMessage::Open);
    if icon_button(
        ui,
        CAN_ASK_FOR_A_DOCUMENT,
        sharp,
        Glyph {
            icon: Icon::Open,
            hint: &open,
        },
    )
    .clicked()
    {
        app.request_open();
        requests.open_dialog = true;
    }
    let hint = app.text(ReaderMessage::CloseHint);
    if icon_button(
        ui,
        app.is_open(),
        sharp,
        Glyph {
            icon: Icon::Close,
            hint: &hint,
        },
    )
    .clicked()
    {
        app.close();
    }
}

/// The view switcher at the centre of the header bar: search results, the
/// signature report, the annotation list. Three is the range the pattern gives,
/// and they are exclusive — plain toggles would let a reader end up with two.
fn draw_view_switcher(ui: &mut Ui, app: &mut App, shown: usize) {
    let sharp = app.high_contrast();
    let open = app.is_open();
    let views = [
        (
            ReaderMessage::SearchToggle,
            Icon::Search,
            app.search_heading(),
        ),
        (
            ReaderMessage::SignaturesToggle,
            Icon::Signatures,
            app.signatures_heading(),
        ),
        (
            ReaderMessage::AnnotationsToggle,
            Icon::Annotations,
            app.annotations_heading(),
        ),
    ];
    // The three are one control, so they sit on one plate: a switcher reads as
    // a set of choices rather than as three buttons that happen to be adjacent.
    let (shown_views, folded_views) = views.split_at(shown.min(views.len()));
    pair(ui, |ui| {
        for (key, icon, hint) in shown_views {
            let chosen = view_showing(app, *key);
            if icon_toggle(ui, open, sharp, chosen, Glyph { icon: *icon, hint }).clicked() {
                show_view(app, *key);
            }
        }
        // The panel on screen is one whose switch did not fit, so the switcher
        // would show three unpressed buttons while a panel is open. Marking the
        // one showing says where the panel came from and which it is.
        let Some((_, icon, hint)) = folded_views
            .iter()
            .find(|(key, ..)| open && view_showing(app, *key))
        else {
            return;
        };
        let plate = ui.visuals().selection.bg_fill;
        ui.scope(|ui| {
            ui.visuals_mut().widgets.active.bg_fill = plate;
            ui.visuals_mut().widgets.active.weak_bg_fill = plate;
            icon_toggle(ui, true, sharp, true, Glyph { icon: *icon, hint });
        });
    });
}

/// Whether the view a switcher label names is the one on screen.
fn view_showing(app: &App, key: ReaderMessage) -> bool {
    match key {
        ReaderMessage::SearchToggle => app.search_open(),
        ReaderMessage::SignaturesToggle => app.signatures_open(),
        _ => app.annotations_open(),
    }
}

/// Show one view, or none when the reader picks the one already showing. The
/// three are exclusive, so picking one closes the other two rather than
/// leaving the reader with panels they did not ask for.
fn show_view(app: &mut App, key: ReaderMessage) {
    let (search, signatures, annotations) = match key {
        ReaderMessage::SearchToggle => (!app.search_open(), false, false),
        ReaderMessage::SignaturesToggle => (false, !app.signatures_open(), false),
        _ => (false, false, !app.annotations_open()),
    };
    app.set_search_open(search);
    app.set_signatures_open(signatures);
    app.set_annotations_open(annotations);
}

/// The main menu: settings, then the language, then shortcuts and the build's
/// name — the shape the guideline gives a primary menu — and no close item,
/// since the header bar's close button is the one that closes a document.
/// `switched` is how many of the view switches the header had room for; the
/// rest are reachable from here, so a narrow window loses a button and not a view.
fn draw_main_menu(ui: &mut Ui, app: &mut App, switched: usize) {
    let label = app.text(ReaderMessage::MainMenu);
    let hint = app.text(ReaderMessage::MainMenuHint);
    let sharp = app.high_contrast();
    // A menu button always draws its label, so the glyph goes in as a picture
    // and the name lives in the tooltip, which is what a screen reader reads.
    let ink = theme::colour(ui.ctx(), sharp, Token::Text);
    let side = icons::box_size(1.0);
    let handle = icons::texture(ui.ctx(), Icon::Menu, ink);
    let image = egui::Image::from_texture(egui::load::SizedTexture::new(handle.id(), side))
        .fit_to_exact_size(side);
    let button = egui::Button::image(image).frame(false).min_size(side);
    let response = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        draw_menu_entries(ui, app, switched);
    });
    let response = response.0.on_hover_text(&hint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
}

/// The views the header bar could not show, as named entries. A switch with no
/// room is a view a reader cannot reach unless it is here, and the header bar
/// carries no claim about which views a document has.
fn draw_folded_views(ui: &mut Ui, app: &mut App, shown: usize) {
    let views = [
        (ReaderMessage::SearchToggle, app.search_heading()),
        (ReaderMessage::SignaturesToggle, app.signatures_heading()),
        (ReaderMessage::AnnotationsToggle, app.annotations_heading()),
    ];
    let mut any = false;
    for (key, heading) in views.into_iter().skip(shown) {
        any = true;
        let showing = view_showing(app, key);
        if ui.selectable_label(showing, heading).clicked() {
            show_view(app, key);
        }
    }
    if any {
        ui.add_space(theme::Space::Medium.points());
    }
}

/// What the main menu holds, in the guideline's order.
fn draw_menu_entries(ui: &mut Ui, app: &mut App, switched: usize) {
    draw_folded_views(ui, app, switched);
    draw_settings_entries(ui, app);
    ui.add_space(theme::Space::Medium.points());
    draw_language_entries(ui, app);
    ui.add_space(theme::Space::Medium.points());
    // Comparing takes a second revision of the open file, so it belongs with
    // the document rather than with the interface, and it disables on the
    // platform whose file dialog Android does not have.
    let compare = app.i18n().text(MessageId::ComparePanelTitle);
    if ui
        .add_enabled(
            app.is_open() && HAS_FILE_DIALOG,
            egui::Button::new(&compare),
        )
        .clicked()
    {
        app.set_comparison_open(true);
    }
    ui.add_space(theme::Space::Medium.points());
    // A menu item is a control, so it is drawn as one, in the text colour and
    // at the body size. Only the two labels that name a group are dim.
    let shortcuts = app.text(ReaderMessage::KeyboardShortcuts);
    ui.add(egui::Button::new(&shortcuts));
    let version = env!("CARGO_PKG_VERSION");
    let about = app.text_with(ReaderMessage::About, [("version", version.into())]);
    ui.add(egui::Button::new(&about));
}

/// The appearance entries, inside the main menu rather than in a menu of their
/// own: the guideline gives an app one primary menu, and a second one beside it
/// would be a second place to look for the same kind of thing.
fn draw_settings_entries(ui: &mut Ui, app: &mut App) {
    let mut theme = app.theme();
    let mut size = app.text_size();
    let mut sharp = app.high_contrast();
    ui.label(toned(
        ui,
        app,
        app.theme_label(),
        Token::Dim,
        Style::Caption,
    ));
    for choice in ThemeChoice::ALL {
        let name = app.theme_name(choice);
        ui.selectable_value(&mut theme, choice, name);
    }
    ui.add_space(theme::Space::Medium.points());
    let heading = app.text_size_label();
    ui.label(toned(ui, app, heading, Token::Dim, Style::Caption));
    for choice in TextSize::ALL {
        let name = app.text_size_name(choice);
        ui.selectable_value(&mut size, choice, name);
    }
    ui.add_space(theme::Space::Medium.points());
    let name = app.high_contrast_label();
    let hint = app.high_contrast_hint();
    ui.checkbox(&mut sharp, name).on_hover_text(&hint);
    // Written back after the entries, so a choice the reader made this frame is
    // the one `App` holds; the state machine is still the only place it lives.
    app.set_theme(theme);
    app.set_text_size(size);
    app.set_high_contrast(sharp);
}

/// The locales, in their own names, so a reader who cannot read the menu's
/// current language can still find theirs. Radio buttons rather than a nested
/// submenu: the guideline asks not to nest menus.
fn draw_language_entries(ui: &mut Ui, app: &mut App) {
    let mut current = app.locale();
    ui.label(toned(
        ui,
        app,
        app.text(ReaderMessage::Language),
        Token::Dim,
        Style::Caption,
    ));
    for locale in app.locales() {
        let option = app.locale_name(&locale);
        ui.radio_value(&mut current, locale, &option);
    }
    app.set_locale(current);
}

/// How the window arranges the pages on screen: one, two facing, or the whole
/// document scrolling. All three are one decision about the page, so they are
/// one group of three glyphs rather than three labelled buttons.
fn draw_layout_switcher(ui: &mut Ui, app: &mut App, shown: usize) {
    let sharp = app.high_contrast();
    for (key, icon, layout) in [
        (
            ReaderMessage::LayoutSingle,
            Icon::SinglePage,
            Layout::Single,
        ),
        (
            ReaderMessage::LayoutDouble,
            Icon::FacingPages,
            Layout::Facing,
        ),
        (
            ReaderMessage::LayoutContinuous,
            Icon::Continuous,
            Layout::Continuous,
        ),
    ]
    .into_iter()
    .take(shown)
    {
        let hint = app.text(key);
        let on = app.layout() == layout;
        if icon_toggle(ui, true, sharp, on, Glyph { icon, hint: &hint }).clicked() {
            app.set_layout(layout);
        }
    }
}

/// The document actions that write something: to a file or to paper. They
/// belong to Z2 because they act on the pages on screen, which is what the
/// toolbar is for, and they disable while a job runs so two cannot interleave.
fn draw_exports(ui: &mut Ui, app: &mut App, shown: usize) {
    let busy = app.is_exporting();
    let printing = app.is_printing();
    let open = app.is_open();
    let sharp = app.high_contrast();
    let printing_hint = app.print_label_text();
    // Two formats without the print button read as a group that never had it,
    // and the menu carries no copy, so a bar without room drops all three.
    if shown < EXPORTS {
        return;
    }
    if icon_button(
        ui,
        open && !printing,
        sharp,
        Glyph {
            icon: Icon::Print,
            hint: &printing_hint,
        },
    )
    .clicked()
    {
        app.request_print();
    }
    let pdf = app.text(ReaderMessage::ExportPdf);
    if icon_button(
        ui,
        open && !busy,
        sharp,
        Glyph {
            icon: Icon::ExportPdf,
            hint: &pdf,
        },
    )
    .clicked()
    {
        app.request_export(ExportFormat::Pdf);
    }
    let png = app.text(ReaderMessage::ExportPng);
    if icon_button(
        ui,
        open && !busy,
        sharp,
        Glyph {
            icon: Icon::ExportPng,
            hint: &png,
        },
    )
    .clicked()
    {
        app.request_export(ExportFormat::Png);
    }
}

/// Z2: the one toolbar — how the pages are arranged, which page, how it is
/// scaled, and what to write it out as. Groups are separated by space, as the
/// guideline asks, and the alignment points are the header bar's.
fn draw_toolbar(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.set_style(style.clone());
        let width = ui.available_width();
        let gap = theme::Space::Small.points();
        let groups = theme::Space::Medium.points();
        // One account, in draw order: a group is charged its run and the step
        // after it, drawn whether or not the group survived. The end groups go
        // first, so `left` is what the foldable ones may spend.
        let step = groups;
        let mut left = width - gap;
        // The slider has no plate, but the exports that follow it do, so it is
        // priced like a plated group less the step it does not get.
        let slider_room = SLIDER_ROOM + value_box_overhang(ui);
        let zoomed = left >= slider_room + step;
        left -= if zoomed { slider_room + step } else { 0.0 };
        let exports_price = run(ui, EXPORTS) + plate_margin();
        let exported = if left >= exports_price + step {
            left -= exports_price + step;
            EXPORTS
        } else {
            0
        };
        // The switcher is asked for first, being what the toolbar is for: paging
        // is also a swipe, a scroll and a key, while a reader with no switcher
        // has one view and no way to change it. Paging then takes what is left.
        let switched = fits_into(&mut left, run(ui, 3) + plate_margin(), 3);
        let paged = fits_into(&mut left, run(ui, 2) + plate_margin(), 2);
        let fitted = fits_into(&mut left, run(ui, 2), 2);
        // Measured against what the row spent, not asked of the account: the
        // row draws steps whose group was folded away. The slider gets no step,
        // being last, where one would push the whole row left by twelve.
        let end_width = if zoomed { slider_room } else { 0.0 }
            + if exported > 0 {
                groups + exports_price
            } else {
                0.0
            };
        ui.horizontal(|ui| {
            // The clip is the panel's, two margins narrower than the window,
            // so a control reaching the last point stopped answering the mouse
            // early. Opened out to the window; the layout is left alone.
            let mut clip = ui.clip_rect();
            clip.max.x += panel_inset(ui) / 2.0;
            ui.set_clip_rect(clip);
            ui.add_space(gap);
            group(ui, switched, |ui| draw_layout_switcher(ui, app, switched));
            ui.add_space(groups);
            group(ui, paged, |ui| draw_paging(ui, app, paged));
            ui.add_space(groups);
            let sharp = app.high_contrast();
            // A fit needs a page to fit; the button disables rather than
            // vanishes, as the guideline asks of every control in this bar.
            let open = app.is_open();
            let fits = [
                (ReaderMessage::FitWidth, Icon::FitWidth, Fit::Width),
                (ReaderMessage::FitPage, Icon::FitPage, Fit::Page),
            ];
            for (key, icon, choice) in fits.into_iter().take(fitted) {
                let hint = app.text(key);
                let on = app.fit() == Some(choice);
                if icon_toggle(ui, open, sharp, on, Glyph { icon, hint: &hint }).clicked() {
                    app.set_fit(choice);
                }
            }
            // The blank is the row's, not the account's: the row knows what it
            // has unspent, and a group's step is drawn even when the group it
            // follows was folded away, which `left` does not carry.
            let blank = ui.available_width() - end_width - value_box_overhang(ui);
            ui.add_space(blank.max(gap));
            draw_exports(ui, app, exported);
            if exported > 0 && zoomed {
                ui.add_space(groups);
            }
            // The last widget on a row is charged a step it cannot spend, so the
            // row gives it back here rather than letting it push the slider off.
            ui.spacing_mut().item_spacing.x = 0.0;
            draw_zoom(ui, app, zoomed);
        });
    });
}

/// The zoom slider. It is the one control in either bar that cannot become an
/// icon, so a bar too narrow for it does not draw it: the toolbar keeps the
/// exports instead, since a reader can pinch to zoom and cannot pinch to export.
fn draw_zoom(ui: &mut Ui, app: &mut App, shown: bool) {
    if !shown {
        return;
    }
    let sharp = app.high_contrast();
    // Whole percent, the same number the label shows: a slider claiming 133.3
    // beside a label reading 133 is two answers.
    let mut percent = (app.zoom() * 100.0).round();
    // The catalogue entry carries a percent placeholder, so the slider is
    // labelled with the value already substituted.
    let text = app.zoom_label();
    // The handle is filled from the widget background, which the window sets to
    // the panel, so it drew as an outline around nothing: a dim fill, for this
    // widget only, gives it a body.
    let grip = theme::colour(ui.ctx(), sharp, Token::Dim);
    // The track narrows before the control is dropped: on a phone the slider is
    // the only way to zoom without pinching. A 100-point track beside a 41-point
    // value box is 154 points, more than a 320dp bar has once its groups are in.
    let room = ui.available_width().max(MIN_TRACK);
    let track = (room - SLIDER_CHROME).clamp(MIN_TRACK, MAX_TRACK);
    let changed = ui
        .scope(|ui| {
            ui.visuals_mut().widgets.inactive.bg_fill = grip;
            ui.spacing_mut().slider_width = track;
            ui.add(
                egui::Slider::new(&mut percent, MIN_PERCENT..=MAX_PERCENT)
                    .text(text)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
            .changed()
        })
        .inner;
    if changed {
        app.set_zoom(percent.round() / 100.0);
    }
}

/// The two paging arrows, as glyphs rather than as the inline triangles they
/// used to be. A reader meeting an arrow needs no label; the tooltip says which
/// way in words, for the reader who wants it.
fn draw_paging(ui: &mut Ui, app: &mut App, shown: usize) {
    let sharp = app.high_contrast();
    // The two arrows are one group, so a strip with room for one is a strip
    // with room for both: half of a paging pair pages nowhere.
    if shown < 2 {
        return;
    }
    let previous = app.text(ReaderMessage::PrevPage);
    if icon_button(
        ui,
        app.has_previous(),
        sharp,
        Glyph {
            icon: Icon::Previous,
            hint: &previous,
        },
    )
    .clicked()
    {
        app.previous_page();
    }
    let next = app.text(ReaderMessage::NextPage);
    if icon_button(
        ui,
        app.has_next(),
        sharp,
        Glyph {
            icon: Icon::Next,
            hint: &next,
        },
    )
    .clicked()
    {
        app.next_page();
    }
}

/// Z5: the search panel, on the left because results are a list.
/// It is drawn before the canvas, so the canvas measures the space left for
/// pages rather than the whole window.
fn draw_search(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    if !app.search_open() || !app.is_open() {
        return;
    }
    egui::Panel::left("search").show(ui, |ui| {
        ui.set_style(style.clone());
        let title = app.search_panel_title();
        ui.label(toned(ui, app, title, Token::Text, Style::Title1));
        let mut query = app.query().to_string();
        let placeholder = app.search_placeholder();
        let response = ui.add(
            egui::TextEdit::singleline(&mut query)
                .id(shortcuts::search_box_id())
                .hint_text(placeholder)
                .desired_width(f32::INFINITY),
        );
        if response.changed() {
            app.set_query(&query);
        }
        ui.add_enabled_ui(!app.hits().is_empty(), |ui| {
            ui.horizontal(|ui| {
                let sharp = app.high_contrast();
                let previous = app.text(ReaderMessage::PrevHit);
                let glyph = Glyph {
                    icon: Icon::Previous,
                    hint: &previous,
                };
                if icon_button(ui, true, sharp, glyph).clicked() {
                    app.previous_hit();
                }
                let next = app.text(ReaderMessage::NextHit);
                let glyph = Glyph {
                    icon: Icon::Next,
                    hint: &next,
                };
                if icon_button(ui, true, sharp, glyph).clicked() {
                    app.next_hit();
                }
                let count = app.search_count_label();
                ui.label(toned(ui, app, count, Token::Dim, Style::Caption));
            });
        });
        if app.no_match() {
            let notice = app.no_match_label();
            ui.label(toned(ui, app, notice, Token::Dim, Style::Body));
        }
        ui.add_space(theme::Space::Medium.points());
        // The hit list, so a reader can see where they all are rather than
        // stepping blind. The selected one is the one the canvas outlines.
        egui::ScrollArea::vertical()
            .id_salt("search-hits")
            .show(ui, |ui| {
                let selected = app.hit_index();
                let hits = app.hits().to_vec();
                for (index, hit) in hits.iter().enumerate() {
                    let text = app.hit_label(index, &hit.snippet);
                    if ui.selectable_label(selected == Some(index), text).clicked() {
                        app.show_hit(index);
                    }
                }
            });
    });
}

/// Z6: the signature panel, also on the left while it is open.
/// It reports what `verify` actually returned rather than a bare valid-or-not,
/// which is what a reader deciding to trust a document needs to see.
fn draw_signatures(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    if !app.signatures_open() || !app.is_open() {
        return;
    }
    egui::Panel::left("signatures").show(ui, |ui| {
        ui.set_style(style.clone());
        let heading = app.signatures_heading();
        ui.label(toned(ui, app, heading, Token::Text, Style::Title1));
        let count = app.signature_count_label();
        ui.label(toned(ui, app, count, Token::Dim, Style::Caption));
        ui.add_space(theme::Space::Medium.points());
        egui::ScrollArea::vertical()
            .id_salt("signature-list")
            .show(ui, |ui| {
                let reports = app.signatures().to_vec();
                if reports.is_empty() {
                    // A document with no signatures is not a failure; it is a
                    // fact, and the count line above already states it.
                    return;
                }
                for report in &reports {
                    let verdict = app.signature_status_label(report);
                    // The glyph and the word say the same thing, in the colour
                    // that carries the reading without a second sentence.
                    let (token, icon) = match report.verdict {
                        Verdict::Valid => (Token::Valid, Icon::Valid),
                        Verdict::Tampered => (Token::Tampered, Icon::Tampered),
                        // Amber, not red: unknown is not the same as wrong.
                        Verdict::Unverified => (Token::Unverified, Icon::Unverified),
                        // The proof holds; the issuer is a policy question.
                        Verdict::Untrusted => (Token::Unverified, Icon::Untrusted),
                    };
                    ui.horizontal(|ui| {
                        icons::show_toned(ui, icon, colour(ui, app, token));
                        ui.label(toned(ui, app, verdict, token, Style::Title4));
                    });
                    let details = [
                        app.signature_signer_label(report),
                        app.signature_time_label(report),
                        app.signature_issuer_label(report),
                        app.signature_validity_label(report),
                        app.signature_algorithm_label(report),
                        // The seal's text is what a reader checks against the
                        // paper in front of them, so it is stated before the
                        // question of how much of the file the proof covers.
                        app.signature_seal_label(report),
                        Some(app.signature_coverage_label(report)),
                    ];
                    for line in details.into_iter().flatten() {
                        ui.label(toned(ui, app, line, Token::Dim, Style::Caption));
                    }
                    // A file no signature covers is the one thing in the
                    // package that could have changed without the proof
                    // noticing, so it is named rather than counted.
                    for loose in &report.details.uncovered {
                        ui.label(toned(
                            ui,
                            app,
                            loose.clone(),
                            Token::Unverified,
                            Style::Caption,
                        ));
                    }
                    ui.add_space(theme::Space::Medium.points());
                }
            });
    });
}

/// Z7: the annotation panel. Clause 15 keeps annotations apart from the pages
/// they mark up, so the panel lists them by page and turns to the one picked:
/// a list nothing links to would be an index of places the reader cannot go.
fn draw_annotations(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    if !app.annotations_open() || !app.is_open() {
        return;
    }
    egui::Panel::left("annotations").show(ui, |ui| {
        ui.set_style(style.clone());
        let heading = app.annotations_heading();
        ui.label(toned(ui, app, heading, Token::Text, Style::Title1));
        let count = app.annotation_count_label();
        ui.label(toned(ui, app, count, Token::Dim, Style::Caption));
        if app.annotation_count() > 0 {
            ui.horizontal(|ui| {
                let sharp = app.high_contrast();
                let previous = app.text(ReaderMessage::PrevAnnotation);
                let glyph = Glyph {
                    icon: Icon::Previous,
                    hint: &previous,
                };
                if icon_button(ui, true, sharp, glyph).clicked() {
                    app.previous_annotation();
                }
                let next = app.text(ReaderMessage::NextAnnotation);
                let glyph = Glyph {
                    icon: Icon::Next,
                    hint: &next,
                };
                if icon_button(ui, true, sharp, glyph).clicked() {
                    app.next_annotation();
                }
            });
        }
        ui.add_space(theme::Space::Medium.points());
        egui::ScrollArea::vertical()
            .id_salt("annotation-list")
            .show(ui, |ui| {
                if app.annotation_count() == 0 {
                    // A document with no annotations is not a failure; it is a
                    // fact, and the count line above already states it.
                    let notice = app.no_annotations_label();
                    ui.label(toned(ui, app, notice, Token::Dim, Style::Body));
                    return;
                }
                let selected = app.annotation_index();
                for index in 0..app.annotation_count() {
                    let text = app.annotation_label(index);
                    if ui.selectable_label(selected == Some(index), text).clicked() {
                        app.show_annotation(index);
                    }
                }
            });
    });
}

/// Z8: the comparison panel. Two revisions of one file are compared over what
/// each draws, so the panel lists the pages that moved and turns to the first.
fn draw_comparison(ui: &mut Ui, app: &mut App, requests: &mut WindowRequests, style: &egui::Style) {
    if !app.comparison_open() || !app.is_open() {
        return;
    }
    egui::Panel::left("comparison").show(ui, |ui| {
        ui.set_style(style.clone());
        let heading = app.comparison_heading();
        ui.label(toned(ui, app, heading, Token::Text, Style::Title1));
        let pick = app.i18n().text(MessageId::CompareOpenSecond);
        let glyph = Glyph {
            icon: Icon::Open,
            hint: &pick,
        };
        if icon_button(ui, true, app.high_contrast(), glyph).clicked() {
            requests.compare = true;
        }
        let Some(comparison) = app.comparison() else {
            return;
        };
        ui.add_space(theme::Space::Medium.points());
        let summary = app.comparison_summary();
        ui.label(toned(ui, app, summary, Token::Dim, Style::Body));
        if comparison.is_identical() {
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("comparison-list")
            .show(ui, |ui| {
                for (index, _) in app.comparison_changes() {
                    let text = app.comparison_row(index);
                    if ui.selectable_label(false, text).clicked() {
                        app.go_to(index);
                    }
                }
            });
    });
}

/// Z3: the only zone that rasterizes, and the only one that may.
fn draw_canvas(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    ui.set_style(style.clone());
    // What the backend can really hold, not the floor this crate assumes. A
    // driver offering 8,192 can show an A4 page at nearly four times the
    // resolution of one held to 2,048, and the reader is the only place that
    // can ask, because the answer arrives with the frame.
    let side = ui.ctx().input(|input| input.max_texture_side);
    app.set_texture_limit(side as u32);
    // The canvas is the only place that knows how much room there is, so a fit
    // is resolved here and the worker asked for the resolution it produces.
    let available = ui.available_size();
    app.apply_fit((available.x, available.y));
    // The toolkit fills the window with the panel colour, so the well behind
    // the pages is painted here, on the background layer: a panel draws its
    // own fill after its contents.
    let well = ui.available_rect_before_wrap();
    let layer = egui::LayerId::background();
    ui.ctx()
        .layer_painter(layer)
        .rect_filled(well, 0, colour(ui, app, Token::Canvas));
    match app.canvas() {
        Canvas::Empty => {
            let hint = app.drop_hint();
            ui.centered_and_justified(|ui| {
                ui.label(toned(ui, app, hint, Token::Dim, Style::Body));
            });
        }
        // The message itself belongs to Z4; stating it twice would put the
        // same sentence on screen twice. Z3 only says something went wrong.
        Canvas::Failed => {
            let notice = app.error_label();
            ui.label(toned(ui, app, notice, Token::Tampered, Style::Body));
        }
        // The pages on screen are already rasterized or are on their way, so
        // both cases draw the same thing; the pending page is simply absent
        // from the cache this frame.
        Canvas::Pending | Canvas::Blank | Canvas::Page => draw_pages(ui, app),
    }
}

/// Paint every page the layout puts on screen, side by side or stacked.
fn draw_pages(ui: &mut Ui, app: &mut App) {
    let pages = app.visible();
    if pages.is_empty() {
        return;
    }
    let horizontal = app.layout() == Layout::Facing;
    let blank = app.canvas() == Canvas::Blank;
    egui::ScrollArea::both()
        .id_salt("canvas")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let layout = if horizontal {
                egui::Layout::left_to_right(egui::Align::Min)
            } else {
                egui::Layout::top_down(egui::Align::Center)
            };
            ui.with_layout(layout, |ui| {
                for page in pages {
                    draw_one_page(ui, app, page);
                }
            });
        });
    if blank {
        let notice = app.blank_notice();
        ui.label(toned(ui, app, notice, Token::Dim, Style::Body));
    }
}

/// Paint a single cached page, at the size the zoom asks for.
///
/// The page always claims its full size on screen, whatever part of it the
/// raster happens to cover, so the scroll area keeps the range a page that
/// size should have and the marks painted over it land in the right place.
/// A raster of part of the page is drawn in the rectangle that part occupies
/// rather than stretched over the whole sheet.
fn draw_one_page(ui: &mut Ui, app: &mut App, page: usize) {
    let Some(rendered) = app.cached(page) else {
        return;
    };
    let image = rendered.image.clone();
    let window = rendered.window;
    let (width_mm, height_mm) = rendered.size_mm;
    // The page's millimetres become points first, because a zoom of one draws
    // a page at its size in points; skipping that step drew A4 280 wide, as if
    // the sheet were 99 mm across, which is what made 100% look wrong.
    let zoom = app.zoom();
    let size = egui::vec2(
        width_mm * POINTS_PER_MM * zoom,
        height_mm * POINTS_PER_MM * zoom,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    // What the reader can actually see of this page tells the next frame which
    // part to rasterize. Only the current page has a viewport, because only it
    // can be zoomed past the texture limit; see `App::window_mm`.
    if page == app.current_page() && rect.width() > 0.0 && rect.height() > 0.0 {
        let visible = rect.intersect(ui.clip_rect());
        if visible.is_positive() {
            let kx = f64::from(width_mm) / f64::from(rect.width());
            let ky = f64::from(height_mm) / f64::from(rect.height());
            app.set_viewport(sofd_common::Rect::new(
                f64::from(visible.min.x - rect.min.x) * kx,
                f64::from(visible.min.y - rect.min.y) * ky,
                f64::from(visible.width()) * kx,
                f64::from(visible.height()) * ky,
            ));
        }
    }
    // One texture per page and resolution; the name carries both, so the
    // cache and the texture cannot disagree.
    let name = format!("page-{page}");
    let texture = ui.ctx().load_texture(name, image, TextureOptions::LINEAR);
    let target = match window {
        Some(part) if width_mm > 0.0 && height_mm > 0.0 => {
            let sx = rect.width() / width_mm;
            let sy = rect.height() / height_mm;
            egui::Rect::from_min_size(
                rect.min + egui::vec2(part.x as f32 * sx, part.y as f32 * sy),
                egui::vec2(part.w as f32 * sx, part.h as f32 * sy),
            )
        }
        _ => rect,
    };
    // An image widget rather than a painted texture, so the page stays one
    // node in the accessibility tree: a reader using a screen reader is told
    // there is a page here, not a rectangle of colour.
    let source = egui::load::SizedTexture::new(texture.id(), target.size());
    ui.put(target, egui::Image::new(source));
    // Highlights are painted over the page, in the rectangle it occupies on
    // screen, so a spread marks the right page.
    draw_highlights(ui, app, page, rect, size);
    draw_annotations_on(ui, app, page, rect, size);
    // A seal answers to a click, which is how notice 2020 no. 1 says an
    // invoice is checked. The hit test is in page space, so zoom is moot.
    if response.clicked() || response.secondary_clicked() {
        let on_seal = ui
            .ctx()
            .pointer_interact_pos()
            .and_then(|at| point_in_page(app, page, rect, size, at))
            .is_some_and(|(hit_page, point)| app.seal_at(hit_page, point).is_some());
        if on_seal {
            app.set_signatures_open(true);
        }
    }
}

/// Outline the annotation boundary on one page, in the space it was drawn.
/// The boundary is in page space, so it is scaled by the ratio the page was
/// drawn at; the annotation being visited is drawn stronger than the rest.
fn draw_annotations_on(ui: &Ui, app: &App, page: usize, area: egui::Rect, drawn: egui::Vec2) {
    let marks = app.annotations_on(page);
    if marks.is_empty() {
        return;
    }
    let Some(page_box) = app.page_size_mm(page) else {
        return;
    };
    let painter = ui.painter();
    for (annotation, current) in marks {
        let Some(boundary) = annotation.boundary else {
            continue;
        };
        // A mark the document says is hidden is listed but not outlined, so
        // the panel can still say it is there without drawing it.
        if !annotation.visible {
            continue;
        }
        // A boundary is a shape rather than text, so it is held to 3:1 on the
        // paper it is drawn over; weight separates the one being visited.
        let colour = colour(ui, app, Token::Mark);
        let scaled = scale_hit(boundary, page_box, area, drawn);
        painter.rect_stroke(
            scaled,
            0.0,
            egui::Stroke::new(if current { 2.0 } else { 1.0 }, colour),
            egui::StrokeKind::Inside,
        );
    }
}

/// Outline every hit on one page, in the space the page was drawn.
/// The hit rectangles are in page space, so they are scaled by the ratio the
/// page was drawn at, and the current hit is drawn stronger than the rest.
fn draw_highlights(ui: &Ui, app: &App, page: usize, area: egui::Rect, drawn: egui::Vec2) {
    let hits = app.hit_rects_on(page);
    if hits.is_empty() {
        return;
    }
    let Some(page_box) = app.page_size_mm(page) else {
        return;
    };
    let painter = ui.painter();
    for (rect, current) in hits {
        // Presence of a hit and the reader's place among them are told apart by
        // weight rather than by fading to invisibility: a faded outline over
        // paper disagrees with whichever page it lands on.
        let colour = colour(ui, app, Token::Hit);
        let scaled = scale_hit(rect, page_box, area, drawn);
        painter.rect_filled(
            scaled,
            0.0,
            colour.gamma_multiply(if current { 0.20 } else { 0.10 }),
        );
        painter.rect_stroke(
            scaled,
            0.0,
            egui::Stroke::new(if current { 2.0 } else { 1.0 }, colour),
            egui::StrokeKind::Inside,
        );
    }
}

/// Map a hit rectangle from page space onto the space the page was drawn in.
/// A pointer position translated into page space, in millimetres. Hit tests run
/// there rather than in pixels, so a seal answers the same at every zoom, and a
/// point outside the page is rejected rather than clamped onto its edge.
fn point_in_page(
    app: &App,
    page: usize,
    area: egui::Rect,
    drawn: egui::Vec2,
    pointer: egui::Pos2,
) -> Option<(usize, (f64, f64))> {
    let page_box = app.page_size_mm(page)?;
    if !area.contains(pointer) {
        return None;
    }
    let sx = f64::from(page_box.0.max(1.0)) / f64::from(drawn.x.max(1.0));
    let sy = f64::from(page_box.1.max(1.0)) / f64::from(drawn.y.max(1.0));
    let x = f64::from(pointer.x - area.min.x) * sx;
    let y = f64::from(pointer.y - area.min.y) * sy;
    if x < 0.0 || y < 0.0 || x > f64::from(page_box.0) || y > f64::from(page_box.1) {
        return None;
    }
    Some((page, (x, y)))
}

fn scale_hit(
    hit: sofd_common::Rect,
    page_box: (f32, f32),
    area: egui::Rect,
    drawn: egui::Vec2,
) -> egui::Rect {
    let sx = drawn.x / page_box.0.max(1.0);
    let sy = drawn.y / page_box.1.max(1.0);
    let x = area.min.x + hit.x as f32 * sx;
    let y = area.min.y + hit.y as f32 * sy;
    egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(hit.w as f32 * sx, hit.h as f32 * sy),
    )
}

/// Z4: document and page on the left, and a failure when there is one.
fn draw_status(ui: &mut Ui, app: &mut App, style: &egui::Style) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.set_style(style.clone());
        ui.set_style(style.clone());
        ui.horizontal(|ui| {
            let Some(err) = app.error() else {
                draw_status_facts(ui, app);
                return;
            };
            let message = err.message.clone();
            ui.label(toned(ui, app, message, Token::Tampered, Style::Body));
        });
    });
}

/// The other half of Z4: a finished export and print speak first, because they
/// are the newest thing that happened, then the document, then the page.
fn draw_status_facts(ui: &mut Ui, app: &mut App) {
    if let Some(label) = app.export_label() {
        ui.label(toned(ui, app, label, Token::Dim, Style::Caption));
        ui.add_space(theme::Space::Medium.points());
    }
    if let Some(label) = app.print_label() {
        ui.label(toned(ui, app, label, Token::Dim, Style::Caption));
        ui.add_space(theme::Space::Medium.points());
    }
    let info = app.info_label();
    if !info.is_empty() {
        ui.label(toned(ui, app, info, Token::Dim, Style::Caption));
        ui.add_space(theme::Space::Medium.points());
    }
    // A page that drew less than it holds says so here, next to the page count
    // it belongs to. This is the reader being its own first user: the counts
    // come from the rasterizer, so a document with a font nobody has fails
    // visibly on the page it failed on rather than being noticed by eye.
    if let Some(short) = app.short_notice() {
        ui.label(toned(ui, app, short, Token::Unverified, Style::Caption));
        ui.add_space(theme::Space::Medium.points());
    }
    let page = app.page_label();
    ui.label(toned(ui, app, page, Token::Text, Style::Body));
}

/// Translate what the platform handed over into the reader's own type.
///
/// A path is preferred; the web backend has none and sends bytes instead.
fn to_dropped(file: &egui::DroppedFile) -> DroppedFile {
    match &file.path {
        Some(path) => DroppedFile::Path(path.clone()),
        None => DroppedFile::Bytes {
            name: file.name.clone(),
            bytes: file.bytes.as_deref().unwrap_or_default().to_vec(),
        },
    }
}
