use eframe::egui::{self, vec2, CornerRadius, Ui};
use egui_phosphor::regular as icon;
use ytm_api::{AlbumPage, AlbumSummary, ArtistRef, SearchItem};

use crate::state::{AppState, Load, Route, UiAction};
use crate::theme::{self, c_text, c_text_dim};
use crate::views::common;
use crate::widgets;

pub fn show(ui: &mut Ui, app: &mut AppState, id: &str) {
    match app.albums.get(id).cloned().unwrap_or_default() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load this album"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out = Vec::new();
            let ps = app.ps.clone();
            let item = SearchItem::Album(AlbumSummary {
                browse_id: page.browse_id.clone(),
                title: page.title.clone(),
                kind: page.kind.clone(),
                artists: page.artists.clone(),
                year: page.year.clone(),
                thumbnails: page.thumbnails.clone(),
            });
            let saved = app.is_saved(&item);
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    header(ui, &page, saved, &mut out);
                    ui.add_space(18.0);
                    common::track_rows(ui, &page.tracks, &ps, true, &mut out);
                    ui.add_space(24.0);
                });
            for a in out {
                app.run(a);
            }
        }
    }
}

pub fn artist_links(ui: &mut Ui, artists: &[ArtistRef], out: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, a) in artists.iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new(", ").color(c_text_dim()).size(15.0));
            }
            let font = egui::FontId::new(15.0, theme::bold_family());
            match &a.id {
                Some(id) => {
                    if widgets::link_text(ui, &a.name, font, c_text()).clicked() {
                        out.push(UiAction::Go(Route::Artist(id.clone())));
                    }
                }
                None => {
                    ui.label(egui::RichText::new(&a.name).font(font).color(c_text()));
                }
            }
        }
    });
}

fn header(ui: &mut Ui, page: &AlbumPage, saved: bool, out: &mut Vec<UiAction>) {
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        let (rect, _) = ui.allocate_exact_size(vec2(220.0, 220.0), egui::Sense::hover());
        widgets::paint_art(
            ui,
            rect,
            widgets::best_thumbnail(&page.thumbnails),
            CornerRadius::same(theme::CARD_RADIUS),
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let kicker = [Some(page.kind.clone()), page.year.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" • ");
            ui.label(egui::RichText::new(kicker).color(c_text_dim()).size(13.0));
            ui.label(theme::bold(&page.title).size(34.0).color(c_text()));
            artist_links(ui, &page.artists, out);
            ui.label(
                egui::RichText::new(&page.stats)
                    .color(c_text_dim())
                    .size(13.0),
            );
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
                if widgets::action_button(ui, icon::PLAY, crate::i18n::t("Play"), true).clicked() {
                    out.push(UiAction::PlayTracks(page.tracks.clone(), 0));
                }
                if widgets::action_button(ui, icon::SHUFFLE, crate::i18n::t("Shuffle"), false)
                    .clicked()
                {
                    out.push(UiAction::ShufflePlay(page.tracks.clone()));
                }
                if widgets::action_button(ui, icon::QUEUE, crate::i18n::t("Add to queue"), false)
                    .clicked()
                {
                    out.push(UiAction::Enqueue(page.tracks.clone()));
                }
                let item = SearchItem::Album(AlbumSummary {
                    browse_id: page.browse_id.clone(),
                    title: page.title.clone(),
                    kind: page.kind.clone(),
                    artists: page.artists.clone(),
                    year: page.year.clone(),
                    thumbnails: page.thumbnails.clone(),
                });
                common::save_button(ui, saved, item, out);
            });
        });
    });
}
