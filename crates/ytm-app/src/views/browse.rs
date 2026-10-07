//! Home and Explore: a vertical stack of carousels.

use eframe::egui::{self, Ui};
use egui_phosphor::regular as icon;

use crate::state::{AppState, Load, Route, UiAction};
use crate::theme;
use crate::views::common;
use crate::widgets;

pub fn home(ui: &mut Ui, app: &mut AppState) {
    let load = app.home.clone();
    show(
        ui,
        app,
        &load,
        Page {
            salt: "home",
            title: None,
            quick_links: false,
            personal: true,
            empty: (
                crate::i18n::t("Welcome to Tunebox"),
                crate::i18n::t("Nothing to show yet."),
            ),
        },
    );
}

pub fn explore(ui: &mut Ui, app: &mut AppState) {
    let load = app.explore.clone();
    show(
        ui,
        app,
        &load,
        Page {
            salt: "explore",
            title: None,
            quick_links: true,
            personal: false,
            empty: (
                crate::i18n::t("Explore"),
                crate::i18n::t("Nothing to show yet."),
            ),
        },
    );
}

pub fn new_releases(ui: &mut Ui, app: &mut AppState) {
    let load = app.new_releases.clone();
    show(
        ui,
        app,
        &load,
        Page {
            salt: "new_releases",
            title: Some(crate::i18n::t("New releases")),
            quick_links: false,
            personal: false,
            empty: (
                crate::i18n::t("New releases"),
                crate::i18n::t("Nothing to show yet."),
            ),
        },
    );
}

/// How a page of carousels is presented.
struct Page {
    salt: &'static str,
    /// Big heading above the rows, if the page has its own title.
    title: Option<&'static str>,
    /// "New releases / Charts / Moods & genres" tiles at the top (Explore).
    quick_links: bool,
    /// Shelves built from this device's history and likes go above YouTube's (Home).
    personal: bool,
    empty: (&'static str, &'static str),
}

/// The three main Explore destinations as equal, neutral icon buttons.
/// Returns the button rectangles (used by tests).
fn quick_links(ui: &mut Ui, out: &mut Vec<UiAction>) -> Vec<egui::Rect> {
    const GAP: f32 = 12.0;
    let w = ((ui.available_width() - 16.0 - 2.0 * GAP) / 3.0)
        .floor()
        .max(120.0);
    let mut rects = Vec::new();
    ui.spacing_mut().item_spacing.x = GAP;
    ui.horizontal(|ui| {
        for (glyph, title, route) in [
            (
                icon::MUSIC_NOTES_PLUS,
                crate::i18n::t("New releases"),
                Route::NewReleases,
            ),
            (
                icon::CHART_LINE_UP,
                crate::i18n::t("Charts"),
                Route::Charts(String::new()),
            ),
            (icon::SMILEY, crate::i18n::t("Moods & genres"), Route::Moods),
        ] {
            let r = widgets::icon_tile(ui, glyph, title, w);
            rects.push(r.rect);
            if r.clicked() {
                out.push(UiAction::Go(route));
            }
        }
    });
    ui.add_space(14.0);
    rects
}

fn show(ui: &mut Ui, app: &mut AppState, load: &Load<ytm_api::HomePage>, page: Page) {
    let Page {
        salt,
        title,
        quick_links: with_quick_links,
        personal,
        empty: (empty_title, empty_body),
    } = page;
    match load {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load this page"), e) {
                app.reload();
            }
        }
        Load::Ready(page) if page.sections.is_empty() => {
            common::message(ui, empty_title, empty_body)
        }
        Load::Ready(page) => {
            let mut out: Vec<UiAction> = Vec::new();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    if let Some(title) = title {
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(title)
                                .text_style(egui::TextStyle::Heading)
                                .color(theme::c_text()),
                        );
                        ui.add_space(8.0);
                    }
                    if with_quick_links {
                        let _ = quick_links(ui, &mut out);
                    }
                    if !page.moods.is_empty() {
                        mood_section(ui, &page.moods, &mut out);
                    }
                    if personal && !app.personal.is_empty() {
                        common::sections(ui, "home_personal", &app.personal, &mut out);
                    }
                    // "More like …" shelves after the first one slot in after YouTube's first shelf
                    let late = if personal {
                        app.personal_late.clone()
                    } else {
                        Default::default()
                    };
                    let split = if late.is_empty() {
                        page.sections.len()
                    } else {
                        1.min(page.sections.len())
                    };
                    common::sections(ui, salt, &page.sections[..split], &mut out);
                    common::sections(ui, "home_late", &late, &mut out);
                    common::sections(ui, "home_rest", &page.sections[split..], &mut out);
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}

/// crate::i18n::t("Moods & genres") shortcuts on Explore, with a link to the full page.
fn mood_section(ui: &mut Ui, moods: &[ytm_api::MoodCategory], out: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        widgets::section_title(ui, crate::i18n::t("Moods & genres"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0); // line up with the carousel arrows' visible edge
            if widgets::link_text(
                ui,
                crate::i18n::t("See all"),
                egui::FontId::new(14.0, theme::bold_family()),
                theme::c_text_dim(),
            )
            .clicked()
            {
                out.push(UiAction::Go(Route::Moods));
            }
        });
    });
    // A two-row teaser; crate::i18n::t("See all") opens the full page.
    let _ = common::mood_chips(ui, moods, Some(2), out);
    ui.add_space(14.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, RawInput, Rect};

    fn links_at(width: f32) -> (Vec<Rect>, Vec<UiAction>) {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let (mut rects, mut actions) = (Vec::new(), Vec::new());
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 600.0))),
            ..RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| rects = quick_links(ui, &mut actions));
        out.textures_delta.clear();
        (rects, actions)
    }

    #[test]
    fn three_equal_buttons_fit_the_row_at_any_width() {
        for width in [620.0, 900.0, 1300.0] {
            let (rects, actions) = links_at(width);
            assert_eq!(rects.len(), 3);
            assert!(
                rects
                    .iter()
                    .all(|r| (r.width() - rects[0].width()).abs() < 0.01),
                "equal widths"
            );
            assert!(
                rects
                    .iter()
                    .all(|r| (r.top() - rects[0].top()).abs() < 0.01),
                "one row"
            );
            assert!(rects[2].right() <= width - 8.0, "fits at {width}");
            assert!(rects[0].right() < rects[1].left() && rects[1].right() < rects[2].left());
            assert!(actions.is_empty(), "nothing is clicked by laying out");
        }
    }
}
