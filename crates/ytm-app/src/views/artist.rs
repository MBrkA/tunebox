use eframe::egui::{self, vec2, CornerRadius, Sense, Ui};
use egui_phosphor::regular as icon;
use ytm_api::{ArtistSummary, SearchItem};

use crate::state::{AppState, Load, UiAction};
use crate::theme::{self, c_text, c_text_dim};
use crate::views::common;
use crate::widgets;

pub fn show(ui: &mut Ui, app: &mut AppState, id: &str) {
    match app.artists.get(id).cloned().unwrap_or_default() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load this artist"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out = Vec::new();
            let ps = app.ps.clone();
            let saved = app.is_saved(&SearchItem::Artist(ArtistSummary {
                browse_id: page.browse_id.clone(),
                ..ArtistSummary::default()
            }));
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 28.0;
                        let (rect, _) = ui.allocate_exact_size(vec2(180.0, 180.0), Sense::hover());
                        widgets::paint_art(
                            ui,
                            rect,
                            widgets::best_thumbnail(&page.thumbnails),
                            CornerRadius::same(90),
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            ui.label(theme::bold(&page.name).size(40.0).color(c_text()));
                            let stats = [
                                page.listeners.clone(),
                                page.subscribers.clone().map(|s| format!("{s} subscribers")),
                            ]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" • ");
                            if !stats.is_empty() {
                                ui.label(egui::RichText::new(stats).color(c_text_dim()).size(13.0));
                            }
                            if !page.description.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&page.description)
                                            .color(c_text_dim())
                                            .size(13.0),
                                    )
                                    .truncate(),
                                );
                            }
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                {
                                    if widgets::action_button(
                                        ui,
                                        icon::PLAY,
                                        crate::i18n::t("Play"),
                                        true,
                                    )
                                    .clicked()
                                    {
                                        out.push(UiAction::PlayTracks(page.top_songs.clone(), 0));
                                    }
                                    if widgets::action_button(
                                        ui,
                                        icon::SHUFFLE,
                                        crate::i18n::t("Shuffle"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        out.push(UiAction::ShufflePlay(page.top_songs.clone()));
                                    }
                                    if widgets::action_button(
                                        ui,
                                        icon::BROADCAST,
                                        crate::i18n::t("Radio"),
                                        false,
                                    )
                                    .clicked()
                                    {
                                        if let Some(t) = page.top_songs.first() {
                                            out.push(UiAction::Radio(t.video_id.clone()));
                                        }
                                    }
                                }
                                common::save_button(
                                    ui,
                                    saved,
                                    SearchItem::Artist(ArtistSummary {
                                        browse_id: page.browse_id.clone(),
                                        name: page.name.clone(),
                                        subtitle: page.listeners.clone().unwrap_or_default(),
                                        thumbnails: page.thumbnails.clone(),
                                    }),
                                    &mut out,
                                );
                            });
                        });
                    });
                    if !page.top_songs.is_empty() {
                        widgets::section_title(ui, crate::i18n::t("Top songs"));
                        common::track_rows(ui, &page.top_songs, &ps, true, &mut out);
                    }
                    ui.add_space(6.0);
                    common::sections(ui, "artist", &page.sections, &mut out);
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}
