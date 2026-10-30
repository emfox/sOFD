//! Draws every glyph into one window, so the set can be looked at as a set.
//! It is a development aid, not a shipped view.

use eframe::egui;
use sofd_egui::icons::{self, Icon};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 460.0]),
        ..Default::default()
    };
    eframe::run_ui_native("sOFD icon set", options, |ui, _frame| {
        let scale = 3.0;
        let cell = icons::box_size(scale).x + 24.0;
        ui.horizontal_wrapped(|ui| {
            for icon in Icon::ALL {
                ui.allocate_ui(egui::vec2(cell, cell + 18.0), |ui| {
                    ui.vertical_centered(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(icons::box_size(scale), egui::Sense::hover());
                        icons::paint(ui.painter(), rect, icon, ui.visuals().text_color());
                        ui.label(icon.theme_name().trim_end_matches("-symbolic"));
                    });
                });
            }
        });
    })
}
