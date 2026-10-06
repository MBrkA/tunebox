//! Charts: per-country rows (video charts, top artists, …) with a country picker.

use eframe::egui::{self, Ui};
use ytm_api::ChartsPage;

use crate::state::{AppState, Load, Route, UiAction};
use crate::theme::c_text;
use crate::views::common;

pub fn page(ui: &mut Ui, app: &mut AppState, country: &str) {
    match app.charts.get(country).cloned().unwrap_or_default() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load the charts"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out: Vec<UiAction> = Vec::new();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(crate::i18n::t("Charts"))
                                .text_style(egui::TextStyle::Heading)
                                .color(c_text()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(14.0);
                            country_picker(ui, &page, &mut out);
                        });
                    });
                    ui.add_space(8.0);
                    common::sections(ui, &format!("charts-{country}"), &page.sections, &mut out);
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}

/// Dropdown of countries ("Global" first); choosing one opens that country's charts.
fn country_picker(ui: &mut Ui, page: &ChartsPage, out: &mut Vec<UiAction>) {
    let current = page
        .country
        .as_ref()
        .map_or(crate::i18n::t("Country"), |c| c.name.as_str())
        .to_owned();
    egui::ComboBox::from_id_salt("charts_country")
        .selected_text(egui::RichText::new(current).strong())
        .width(210.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            common::style_menu(ui);
            // "Global" on top, the rest alphabetically as YouTube lists them.
            let (global, rest): (Vec<_>, Vec<_>) =
                page.countries.iter().partition(|c| c.code == "ZZ");
            for c in global.into_iter().chain(rest) {
                let selected = page.country.as_ref().is_some_and(|cur| cur.code == c.code);
                if ui.selectable_label(selected, &c.name).clicked() && !selected {
                    out.push(UiAction::Go(Route::Charts(c.code.clone())));
                }
            }
        });
}
