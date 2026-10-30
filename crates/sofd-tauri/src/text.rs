//! Text for the webview: every sentence the window shows, taken from the same
//! catalogue the `egui` reader reads.
//!
//! A shell that spelled its own labels would be a second interface translated
//! separately, disagreeing the first time a word was reworded. The window asks
//! for a message id; this maps it back, so a missing entry is a failed test.

use serde::Serialize;
use sofd_session::{App, ReaderMessage};

/// One message: the name the window asks with, and the words it gets back.
#[derive(Debug, Serialize)]
pub struct Message {
    /// The variant's name, which the window uses as its key.
    pub id: &'static str,
    /// The translated text.
    pub text: String,
}

/// Every message the window can ask for, named.
///
/// The list is written out rather than derived, because a message id is part of
/// the window's interface: renaming a variant renames a key the front end holds,
/// and a compile error here is cheaper than an empty label at run time.
pub fn messages(app: &App) -> Vec<Message> {
    [
        ("Open", ReaderMessage::Open),
        ("Close", ReaderMessage::Close),
        ("DropHint", ReaderMessage::DropHint),
        ("OpenHint", ReaderMessage::OpenHint),
        ("Info", ReaderMessage::Info),
        ("PageOf", ReaderMessage::PageOf),
        ("Zoom", ReaderMessage::Zoom),
        ("FitWidth", ReaderMessage::FitWidth),
        ("FitPage", ReaderMessage::FitPage),
        ("LayoutSingle", ReaderMessage::LayoutSingle),
        ("LayoutDouble", ReaderMessage::LayoutDouble),
        ("LayoutContinuous", ReaderMessage::LayoutContinuous),
        ("BlankPage", ReaderMessage::BlankPage),
        ("ShortGlyphs", ReaderMessage::ShortGlyphs),
        ("ShortFonts", ReaderMessage::ShortFonts),
        ("ShortImages", ReaderMessage::ShortImages),
        ("ErrorLabel", ReaderMessage::ErrorLabel),
        ("SearchTitle", ReaderMessage::SearchTitle),
        ("SearchPanelTitle", ReaderMessage::SearchPanelTitle),
        ("NoMatch", ReaderMessage::NoMatch),
        ("Export", ReaderMessage::Export),
        ("ExportPdf", ReaderMessage::ExportPdf),
        ("ExportPng", ReaderMessage::ExportPng),
        ("ExportPdfDone", ReaderMessage::ExportPdfDone),
        ("ExportPngDone", ReaderMessage::ExportPngDone),
        ("Print", ReaderMessage::Print),
        ("PrintDone", ReaderMessage::PrintDone),
        ("Settings", ReaderMessage::Settings),
        ("MainMenu", ReaderMessage::MainMenu),
        ("MainMenuHint", ReaderMessage::MainMenuHint),
        ("KeyboardShortcuts", ReaderMessage::KeyboardShortcuts),
        ("About", ReaderMessage::About),
        ("Language", ReaderMessage::Language),
        ("Views", ReaderMessage::Views),
        ("SearchToggle", ReaderMessage::SearchToggle),
        ("SignaturesToggle", ReaderMessage::SignaturesToggle),
        ("AnnotationsToggle", ReaderMessage::AnnotationsToggle),
        ("Theme", ReaderMessage::Theme),
        ("ThemeSystem", ReaderMessage::ThemeSystem),
        ("ThemeLight", ReaderMessage::ThemeLight),
        ("ThemeDark", ReaderMessage::ThemeDark),
        ("TextSize", ReaderMessage::TextSize),
        ("TextSizeNormal", ReaderMessage::TextSizeNormal),
        ("TextSizeLarge", ReaderMessage::TextSizeLarge),
        ("TextSizeLarger", ReaderMessage::TextSizeLarger),
        ("HighContrast", ReaderMessage::HighContrast),
        ("HighContrastHint", ReaderMessage::HighContrastHint),
        ("PrevPage", ReaderMessage::PrevPage),
        ("NextPage", ReaderMessage::NextPage),
        ("PrevHit", ReaderMessage::PrevHit),
        ("NextHit", ReaderMessage::NextHit),
        ("PrevAnnotation", ReaderMessage::PrevAnnotation),
        ("NextAnnotation", ReaderMessage::NextAnnotation),
        ("CloseHint", ReaderMessage::CloseHint),
        ("VerdictValid", ReaderMessage::VerdictValid),
        ("VerdictTampered", ReaderMessage::VerdictTampered),
        ("VerdictUnverified", ReaderMessage::VerdictUnverified),
        ("VerdictUntrusted", ReaderMessage::VerdictUntrusted),
    ]
    .into_iter()
    .map(|(id, message)| Message {
        id,
        text: app.text(message),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every id the window asks for resolves to words. An empty label is the
    /// failure this catches: the catalogue lost an entry and nothing else would
    /// say so until a reader saw a blank button. English spells one entry
    /// `Export`, which is also its key, so the check is for words and not for
    /// the key failing to appear.
    #[test]
    fn every_message_has_words() {
        let app = App::new();
        for message in messages(&app) {
            assert!(
                !message.text.trim().is_empty(),
                "{} came back empty",
                message.id
            );
        }
    }

    /// The catalogue is the one the `egui` reader reads, so the two shells
    /// speak the same words rather than two translations of one word. Checked
    /// on three ids the window draws constantly, since the point is the source
    /// and not the count.
    #[test]
    fn the_words_come_from_the_catalogue() {
        let app = App::new();
        let sent = messages(&app);
        let find = |id: &str| {
            sent.iter()
                .find(|m| m.id == id)
                .map(|m| m.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(find("Open"), app.text(ReaderMessage::Open));
        assert_eq!(find("Close"), app.text(ReaderMessage::Close));
        assert_eq!(find("PageOf"), app.text(ReaderMessage::PageOf));
    }
}
