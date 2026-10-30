// The window: it draws what the session reports and asks for changes.
//
// Nothing here decides what the reader sees. The state, the strings, the zoom,
// the fit and the icon set all arrive from the host; this file turns them into
// pixels and turns a click back into a name. That is the same division the
// `egui` shell keeps, and it is why a second shell needed no new decision —
// only a second way to paint one.

const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

/// Every sentence the window shows, keyed by the name the host gives it.
let text = {};

/// Every glyph, keyed by name, already parsed into a `Path2D` on the 16-unit
/// grid the theme draws on. Built once: a glyph is the same drawing whatever
/// colour it is filled with, because the fill is `currentColor`.
let glyphs = {};
let glyphGrid = 16;

/// The last snapshot, kept so a repaint after a scroll needs no round trip.
let state = null;

/// Pages already drawn, keyed by page index, so scrolling does not ask the
/// engine for geometry it already sent.
const drawn = new Map();

/// Run a command and hold its failure where a reader can see it, rather than
/// letting a rejected promise disappear into the console.
async function call(name, args = {}) {
  try {
    return await invoke(name, args);
  } catch (error) {
    showError(String(error));
    return null;
  }
}

function showError(message) {
  const slot = document.getElementById("error");
  slot.textContent = message;
}

function t(id) {
  return text[id] ?? id;
}

/// Build a control's accessible name and tooltip from one string, so the two
/// cannot drift apart: a glyph carries no words, and this is the one place the
/// two are set together.
function describe(element, label) {
  element.setAttribute("aria-label", label);
  element.title = label;
  element.dataset.label = label;
}

/// Draw a glyph into a button, on the theme's 16-unit grid at whatever size the
/// box is. `currentColor` is what makes one drawing follow every theme.
function paintGlyph(element, name) {
  const paths = glyphs[name];
  if (!paths) return;
  const size = 20;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${glyphGrid} ${glyphGrid}`);
  svg.setAttribute("class", "glyph");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  for (const d of paths) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    path.setAttribute("fill-rule", "nonzero");
    svg.append(path);
  }
  element.replaceChildren(svg);
}

/// A flat button with a glyph, its words in the tooltip and the accessible name.
function iconButton(id, icon, label, onClick) {
  const button = document.getElementById(id);
  if (!button) return null;
  paintGlyph(button, icon);
  describe(button, label);
  if (onClick) button.addEventListener("click", onClick);
  return button;
}

/// One toggle in a group of exclusive choices, drawn with its glyph and words.
function toggle(parent, { id, icon, label, on, onClick }) {
  const button = document.createElement("button");
  button.className = "control";
  button.id = id;
  button.setAttribute("aria-pressed", String(on));
  paintGlyph(button, icon);
  describe(button, label);
  button.addEventListener("click", onClick);
  parent.append(button);
  return button;
}

/// The icons the layout names in markup, so a static button is never a blank.
function staticIcons() {
  for (const element of document.querySelectorAll("[data-icon]")) {
    paintGlyph(element, element.dataset.icon);
  }
}

async function boot() {
  const [set, sentences, snapshot] = await Promise.all([
    invoke("glyphs"),
    invoke("messages"),
    invoke("snapshot"),
  ]);
  glyphGrid = set.size;
  glyphs = Object.fromEntries(set.glyphs.map((g) => [g.id, g.paths]));
  text = Object.fromEntries(sentences.map((m) => [m.id, m.text]));
  state = snapshot;

  staticIcons();
  buildChrome();
  apply(snapshot);

  // A drop is the same call a menu would make, so the two cannot diverge.
  await listen("sofd://drop", async (event) => {
    await call("open", { path: event.payload });
    await refresh();
  });
}

/// The parts of the window that depend on the strings: three view toggles, a
/// menu, and the toolbar's own names.
function buildChrome() {
  const views = document.getElementById("views");
  views.replaceChildren();
  toggle(views, {
    id: "view-search",
    icon: "Search",
    label: t("SearchToggle"),
    on: state.searchOpen,
    onClick: () => act("toggleSearch"),
  });
  toggle(views, {
    id: "view-signatures",
    icon: "Signatures",
    label: t("SignaturesToggle"),
    on: state.signaturesOpen,
    onClick: () => act("toggleSignatures"),
  });
  toggle(views, {
    id: "view-annotations",
    icon: "Annotations",
    label: t("AnnotationsToggle"),
    on: state.annotationsOpen,
    onClick: () => act("toggleAnnotations"),
  });

  const layouts = document.getElementById("layouts");
  layouts.replaceChildren();
  for (const [id, icon, name] of [
    ["layout-single", "SinglePage", "LayoutSingle"],
    ["layout-facing", "FacingPages", "LayoutDouble"],
    ["layout-continuous", "Continuous", "LayoutContinuous"],
  ]) {
    toggle(layouts, {
      id,
      icon,
      label: t(name),
      on: false,
      onClick: () => send("layout", { name: id.replace("layout-", "") }),
    });
  }

  iconButton("open", "Open", t("Open"), () => act("open"));
  iconButton("close", "Close", t("Close"), () => act("escape"));
  iconButton("previous", "Previous", t("PrevPage"), () => act("previousPage"));
  iconButton("next", "Next", t("NextPage"), () => act("nextPage"));
  iconButton("fit-width", "FitWidth", t("FitWidth"), () =>
    send("fit", { name: "width" }),
  );
  iconButton("fit-page", "FitPage", t("FitPage"), () =>
    send("fit", { name: "page" }),
  );
  iconButton("print", "Print", t("Print"), () => act("print"));
  iconButton("export-pdf", "ExportPdf", t("ExportPdf"), () => exportAs("pdf"));
  iconButton("export-png", "ExportPng", t("ExportPng"), () => exportAs("png"));
  const menuButton = document.getElementById("menu-button");
  describe(menuButton, t("MainMenu"));
  menuButton.addEventListener("click", toggleMenu);

  document.getElementById("zoom").addEventListener("input", (event) => {
    send("zoom", { percent: Number(event.target.value) });
  });
}

/// Ask the host for a change, then redraw. Every control goes through here, so
/// a click and a key press reach the same state.
async function send(command, args) {
  await call(command, args);
  await refresh();
}

async function act(name) {
  await send("act", { name });
}

/// Write the pages on screen out. A PDF is one file and a PNG is one file per
/// page, so the window asks for a path in the first case and a folder in the
/// second; cancelling is not a failure and writes nothing.
function exportAs(format) {
  const destination = window.prompt(
    format === "pdf" ? t("ExportPdfTitle") : t("ExportPngTitle"),
    state?.title?.replace(/\.ofd$/i, "") ?? "document",
  );
  if (!destination) return;
  send("export", { format, destination }).then(refreshReport);
}

/// A finished export is a sentence in the status bar, and it arrives when the
/// worker says so rather than when the click happened.
async function refreshReport() {
  const reports = await call("collect");
  if (!reports || reports.length === 0) return;
  const slot = document.getElementById("notice");
  slot.textContent = reports.join(" ");
}

async function refresh() {
  const snapshot = await call("snapshot");
  if (snapshot) apply(snapshot);
}

/// Draw one snapshot: the appearance first, then the zones in the order the
/// frame flow states, so nothing is painted with the previous theme's colours.
function apply(snapshot) {
  const before = state;
  state = snapshot;
  paintAppearance(snapshot.appearance);
  paintHeader(snapshot, before);
  paintToolbar(snapshot, before);
  paintPanels(snapshot);
  paintStatus(snapshot);
  paintCanvas(snapshot, before);
}

/// 5.1 and 3: appearance is a state setting, applied before the first zone.
function paintAppearance(appearance) {
  const root = document.documentElement;
  const scheme =
    appearance.theme === "system"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : appearance.theme;
  root.dataset.scheme = scheme;
  root.dataset.contrast = appearance.highContrast ? "sharp" : "normal";
  root.style.setProperty("--scale", String(appearance.textScale));
}

function paintHeader(snapshot) {
  for (const [id, on] of [
    ["view-search", snapshot.searchOpen],
    ["view-signatures", snapshot.signaturesOpen],
    ["view-annotations", snapshot.annotationsOpen],
  ]) {
    const button = document.getElementById(id);
    if (button) button.setAttribute("aria-pressed", String(on));
  }
  const close = document.getElementById("close");
  if (close) close.disabled = !snapshot.open;
  document.title = snapshot.title ?? "sOFD";
}

/// 2, Z2: disabled rather than hidden, so the bar does not reflow as the
/// document changes and the reader's muscle memory holds.
function paintToolbar(snapshot) {
  const has = snapshot.open;
  for (const id of ["previous", "next", "fit-width", "fit-page", "print"]) {
    const button = document.getElementById(id);
    if (button) button.disabled = !has;
  }
  const previous = document.getElementById("previous");
  if (previous) previous.disabled = !snapshot.hasPrevious;
  const next = document.getElementById("next");
  if (next) next.disabled = !snapshot.hasNext;

  for (const [id, name] of [
    ["layout-single", "single"],
    ["layout-facing", "facing"],
    ["layout-continuous", "continuous"],
  ]) {
    const button = document.getElementById(id);
    if (button) button.setAttribute("aria-pressed", String(snapshot.layout === name));
  }

  const zoom = document.getElementById("zoom");
  const value = document.getElementById("zoom-value");
  if (zoom && value) {
    zoom.value = String(snapshot.zoomPercent);
    value.textContent = `${snapshot.zoomPercent}%`;
    zoom.disabled = !has;
  }
}

/// 2, Z3: exactly one of five states, decided in the order the spec states.
function paintCanvas(snapshot, before) {
  const hint = document.getElementById("hint");
  const pages = document.getElementById("pages");

  // A document that changed is a document whose geometry is not the old one's,
  // and its pages are not on screen any more either.
  if (!snapshot.open || (before && before.title !== snapshot.title)) {
    drawn.clear();
  }
  if (!snapshot.open) {
    pages.replaceChildren();
    hint.textContent = snapshot.dropHint;
    hint.hidden = false;
    return;
  }
  hint.hidden = true;

  // The pages on screen, one canvas each. They are created once and reused, so
  // a page that has not changed is not redrawn on every snapshot.
  const wanted = snapshot.visible;
  const have = [...pages.children].map((child) => Number(child.dataset.page));
  if (String(have) !== String(wanted)) {
    pages.replaceChildren();
    for (const index of wanted) {
      const frame = document.createElement("div");
      frame.className = "page";
      frame.dataset.page = String(index);
      const canvas = document.createElement("canvas");
      frame.append(canvas);
      pages.append(frame);
    }
  }
  for (const frame of pages.children) {
    const index = Number(frame.dataset.page);
    drawPage(frame, index, snapshot).catch((error) => showError(String(error)));
  }

  // The canvas measures the width actually left, and the session fits the page
  // to it: a fit that follows the window is decided there, not here.
  const zone = document.getElementById("canvas-zone");
  const width = zone.clientWidth - 48;
  const height = zone.clientHeight - 48;
  if (before === null || zone.dataset.width !== String(width) || zone.dataset.height !== String(height)) {
    zone.dataset.width = String(width);
    zone.dataset.height = String(height);
    send("viewport", { width, height });
  }
}

/// One page, drawn as geometry: shapes in millimetres, transformed once into
/// device pixels. A zoom is a transform here, so it costs no new geometry.
async function drawPage(frame, index, snapshot) {
  const geometry = drawn.get(index) ?? (await call("page", { index }));
  if (!geometry) return;
  drawn.set(index, geometry);

  const canvas = frame.querySelector("canvas");
  const ratio = window.devicePixelRatio || 1;
  // The page's on-screen size is the session's zoom, which is a share of its
  // 72 dpi size; the canvas is that size in device pixels, not a stretched
  // bitmap, which is what keeps a zoomed page sharp.
  const cssWidth = (geometry.widthMm / 25.4) * 72 * snapshot.dpi / 72;
  const cssHeight = (geometry.heightMm / 25.4) * 72 * snapshot.dpi / 72;
  frame.style.width = `${cssWidth}px`;
  frame.style.height = `${cssHeight}px`;
  canvas.width = Math.round(cssWidth * ratio);
  canvas.height = Math.round(cssHeight * ratio);

  const ctx = canvas.getContext("2d");
  const scale = canvas.width / geometry.widthMm;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.save();
  ctx.scale(scale, scale);
  // A page's own background is white in every theme, because the document's
  // background is what it is rather than what the reader prefers.
  drawShapes(ctx, geometry, scale);
  // Over the content and inside the same transform, so a mark is placed by the
  // page's own millimetres rather than by a rectangle scaled twice.
  drawMarks(ctx, await call("overlays", { index }));
  ctx.restore();
  frame.dataset.drawn = "true";
}

/// Walk a page's shapes in the order the engine gave them.
function drawShapes(ctx, geometry, scale) {
  let depth = 0;
  for (const shape of geometry.shapes) {
    switch (shape.kind) {
      case "ink": {
        const path = new Path2D(shape.d);
        if (shape.fill) {
          ctx.fillStyle = paint(ctx, shape.fill);
          ctx.fill(path, shape.rule);
        }
        if (shape.stroke) {
          ctx.strokeStyle = paint(ctx, shape.stroke);
          // A stroke width is in millimetres like the geometry, so the
          // transform that takes millimetres to pixels already scales it.
          ctx.lineWidth = shape.style.width;
          ctx.lineCap = shape.style.cap;
          ctx.lineJoin = shape.style.join;
          ctx.miterLimit = shape.style.miterLimit;
          ctx.setLineDash(shape.style.dash);
          ctx.lineDashOffset = shape.style.dashOffset;
          ctx.stroke(path);
          ctx.setLineDash([]);
        }
        break;
      }
      case "clip": {
        ctx.save();
        depth += 1;
        ctx.clip(new Path2D(shape.d), shape.rule);
        break;
      }
      case "clipReset": {
        if (depth > 0) {
          ctx.restore();
          depth -= 1;
        }
        break;
      }
      case "image": {
        drawImage(ctx, shape);
        break;
      }
    }
  }
  // A page that ended inside a clip put the stack back rather than leaving the
  // next page clipped by this one.
  while (depth > 0) {
    ctx.restore();
    depth -= 1;
  }
}

/// The rectangles drawn over the page: the hits search found and the boundary
/// of each annotation. Weight says which one the reader is visiting, and the
/// colour comes from the token the session named, so a theme switch recolours
/// them without asking the engine for the page again.
function drawMarks(ctx, overlays) {
  if (!overlays) return;
  for (const mark of overlays.hits) {
    const colour = css(mark.token);
    ctx.globalAlpha = mark.current ? 0.2 : 0.1;
    ctx.fillStyle = colour;
    ctx.fillRect(mark.x, mark.y, mark.w, mark.h);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = colour;
    ctx.lineWidth = mark.current ? 2 : 1;
    ctx.strokeRect(mark.x, mark.y, mark.w, mark.h);
  }
  for (const mark of overlays.marks) {
    ctx.strokeStyle = css(mark.token);
    ctx.lineWidth = mark.current ? 2 : 1;
    ctx.strokeRect(mark.x, mark.y, mark.w, mark.h);
  }
}

/// A bitmap, placed through the matrix the engine computed for it.
function drawImage(ctx, shape) {
  if (!shape.src) return;
  const image = new Image();
  image.onload = () => {
    ctx.save();
    const [a, b, c, d, e, f] = shape.toPage;
    ctx.transform(a, b, c, d, e, f);
    ctx.drawImage(image, 0, 0, shape.pixelWidth, shape.pixelHeight);
    ctx.restore();
  };
  image.src = shape.src;
}

/// A paint, as something a canvas can set. The colour the engine sends is a
/// token's name rather than a value, so the sheet decides what it looks like:
/// that is what lets a theme or a high-contrast mode recolour a document
/// without asking the engine for the page again.
function paint(ctx, source) {
  if (source.type === "solid") {
    return token(source.css) || source.css;
  }
  const gradient =
    source.type === "linear"
      ? ctx.createLinearGradient(source.x0, source.y0, source.x1, source.y1)
      : ctx.createRadialGradient(
          source.x0,
          source.y0,
          source.r0,
          source.x1,
          source.y1,
          source.r1,
        );
  for (const stop of source.stops) {
    gradient.addColorStop(stop.offset, token(stop.css) || stop.css);
  }
  return gradient;
}

/// A token's value from the sheet, or empty when the name is not one.
function token(name) {
  if (!name.startsWith("--")) return "";
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}

/// A token, resolved, falling back to the name so a missing one draws nothing
/// rather than throwing on every mark of a page.
function css(name) {
  return token(name) || name;
}

/// 2, Z4: the document and the position on the left, the last error winning
/// the left half, because a message that flickers away is one that is missed.
function paintStatus(snapshot) {
  document.getElementById("document").textContent = snapshot.open
    ? (snapshot.title ?? "")
    : t("Info");
  document.getElementById("position").textContent = snapshot.open
    ? snapshot.pageLabel
    : "";
  document.getElementById("notice").textContent = snapshot.notice ?? "";
  const error = document.getElementById("error");
  if (snapshot.error) error.textContent = snapshot.error;
}

/// Z5..Z8: one panel at a time, in the order the session says.
function paintPanels(snapshot) {
  const panel = document.getElementById("panel");
  const open =
    snapshot.searchOpen || snapshot.signaturesOpen || snapshot.annotationsOpen;
  panel.hidden = !open;
  if (!open) return;
  const which = snapshot.searchOpen
    ? "SearchPanelTitle"
    : snapshot.signaturesOpen
      ? "SignaturesToggle"
      : "AnnotationsToggle";
  panel.replaceChildren();
  const heading = document.createElement("h2");
  heading.textContent = t(which);
  panel.append(heading);
}

/// The main menu: appearance, then language, then the shortcuts and the build,
/// in the order the guideline gives a primary menu.
async function toggleMenu() {
  const menu = document.getElementById("menu");
  const button = document.getElementById("menu-button");
  if (!menu.hidden) {
    menu.hidden = true;
    button.setAttribute("aria-expanded", "false");
    return;
  }
  menu.replaceChildren();
  section(t("Theme"));
  for (const [value, label] of [
    ["system", "ThemeSystem"],
    ["light", "ThemeLight"],
    ["dark", "ThemeDark"],
  ]) {
    entry(t(label), state.appearance.theme === value, () =>
      send("settings", { settings: { theme: value } }),
    );
  }
  section(t("TextSize"));
  for (const [value, label] of [
    ["normal", "TextSizeNormal"],
    ["large", "TextSizeLarge"],
    ["larger", "TextSizeLarger"],
  ]) {
    entry(t(label), Math.abs(state.appearance.textScale - scaleOf(value)) < 0.001, () =>
      send("settings", { settings: { textSize: value } }),
    );
  }
  entry(t("HighContrast"), state.appearance.highContrast, () =>
    send("settings", {
      settings: { highContrast: !state.appearance.highContrast },
    }),
  );
  menu.append(document.createElement("hr"));
  section(t("KeyboardShortcuts"));
  section(t("About"));

  menu.hidden = false;
  button.setAttribute("aria-expanded", "true");
  // Picking anything closes the menu, which is what a menu is for.
  menu.addEventListener("click", () => {
    menu.hidden = true;
    button.setAttribute("aria-expanded", "false");
  }, { once: true });
}

function scaleOf(name) {
  return name === "larger" ? 1.3 : name === "large" ? 1.15 : 1;
}

function section(label) {
  const item = document.createElement("li");
  item.className = "heading";
  item.textContent = label;
  document.getElementById("menu").append(item);
}

function entry(label, checked, onClick) {
  const item = document.createElement("li");
  item.setAttribute("role", "menuitemradio");
  item.setAttribute("aria-checked", String(checked));
  item.textContent = label;
  item.addEventListener("click", onClick);
  document.getElementById("menu").append(item);
}

/// Every key press is offered to the session's own table, so which key does
/// what is written down once and the two shells cannot disagree about it. A
/// press a text field took never reaches here: the browser gives it to the
/// field, which is what stops a reader typing a query from turning pages.
window.addEventListener("keydown", async (event) => {
  if (event.target instanceof HTMLInputElement) return;
  const ctrl = event.ctrlKey || event.metaKey;
  await send("press", {
    keyName: event.key,
    ctrl,
    shift: event.shiftKey,
  });
  if (event.key === "Escape") {
    const menu = document.getElementById("menu");
    if (!menu.hidden) {
      menu.hidden = true;
      document.getElementById("menu-button").setAttribute("aria-expanded", "false");
    }
  }
});

/// A window that changes size is a window whose fit changed.
window.addEventListener("resize", () => {
  if (!state) return;
  const zone = document.getElementById("canvas-zone");
  delete zone.dataset.width;
  paintCanvas(state, null);
});

window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (state?.appearance.theme === "system") paintAppearance(state.appearance);
});

boot().catch((error) => showError(String(error)));
