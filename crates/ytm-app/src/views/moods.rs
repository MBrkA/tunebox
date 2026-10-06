//! crate::i18n::t("Moods & genres"): the full list, and a single category's playlists.

use eframe::egui::{self, Ui};

use crate::state::{AppState, Load, UiAction};
use crate::theme::c_text;
use crate::views::common;
use crate::widgets;

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text)
            .text_style(egui::TextStyle::Heading)
            .color(c_text()),
    );
    ui.add_space(8.0);
}

/// Every mood and genre, grouped ("Moods & moments", "Genres").
pub fn page(ui: &mut Ui, app: &mut AppState) {
    match app.moods.clone() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load moods & genres"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out: Vec<UiAction> = Vec::new();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    heading(ui, crate::i18n::t("Moods & genres"));
                    for group in &page.groups {
                        if !group.title.is_empty() {
                            widgets::section_title(ui, &group.title);
                        }
                        let _ = common::mood_chips(ui, &group.categories, None, &mut out);
                        ui.add_space(10.0);
                    }
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}

/// One category: its title and rows of playlists.
pub fn category(ui: &mut Ui, app: &mut AppState, params: &str) {
    match app.mood_pages.get(params).cloned().unwrap_or_default() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load this category"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out: Vec<UiAction> = Vec::new();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    heading(ui, &page.title);
                    common::sections(ui, params, &page.sections, &mut out);
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}
