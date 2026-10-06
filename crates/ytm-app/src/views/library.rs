//! Library (liked songs, saved albums and artists) and the playlists page.
//! Everything here lives on this device; no account is involved.

use eframe::egui::{self, Ui};
use ytm_api::SearchItem;

use crate::local::is_local_id;
use crate::state::{AppState, LibraryTab, UiAction};
use crate::views::common;
use crate::widgets;

fn grid(ui: &mut Ui, items: &[SearchItem], out: &mut Vec<UiAction>) {
    let gap = 14.0;
    let (cols, w) = widgets::grid_layout(ui.available_width(), 184.0, gap);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    for chunk in items.chunks(cols) {
        ui.horizontal_top(|ui| {
            for item in chunk {
                let r = common::item_card(ui, item, w, &[], out);
                match item {
                    // Playlists created on this device.
                    SearchItem::Playlist(p) if is_local_id(&p.playlist_id) => {
                        r.context_menu(|ui| {
                            crate::views::common::style_menu(ui);
                            if ui.button(crate::i18n::t("Rename")).clicked() {
                                out.push(UiAction::RenamePlaylist(p.playlist_id.clone()));
                                ui.close();
                            }
                            if ui.button(crate::i18n::t("Delete playlist")).clicked() {
                                out.push(UiAction::DeletePlaylist(p.playlist_id.clone()));
                                ui.close();
                            }
                        });
                    }
                    // Saved albums, artists and YouTube playlists.
                    _ => {
                        r.context_menu(|ui| {
                            crate::views::common::style_menu(ui);
                            if ui.button(crate::i18n::t("Remove from library")).clicked() {
                                out.push(UiAction::ToggleSaved(item.clone()));
                                ui.close();
                            }
                        });
                    }
                }
            }
        });
    }
}

fn grid_or_message(
    ui: &mut Ui,
    items: &[SearchItem],
    title: &str,
    body: &str,
    out: &mut Vec<UiAction>,
) {
    if items.is_empty() {
        common::message(ui, title, body);
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| grid(ui, items, out));
    }
}

pub fn library(ui: &mut Ui, app: &mut AppState) {
    let mut out = Vec::new();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (tab, label) in [
            (LibraryTab::Songs, crate::i18n::t("Songs")),
            (LibraryTab::Albums, crate::i18n::t("Albums")),
            (LibraryTab::Artists, crate::i18n::t("Artists")),
        ] {
            if widgets::pill(ui, label, app.library_tab == tab).clicked() {
                app.library_tab = tab;
            }
        }
    });
    ui.add_space(12.0);

    match app.library_tab {
        LibraryTab::Songs => {
            let tracks = app.local.liked.clone();
            if tracks.is_empty() {
                common::message(
                    ui,
                    crate::i18n::t("No liked songs yet"),
                    crate::i18n::t("Tap the heart on a song to keep it here."),
                );
            } else {
                let ps = app.ps.clone();
                ui.horizontal(|ui| {
                    if widgets::action_button(
                        ui,
                        egui_phosphor::regular::PLAY,
                        crate::i18n::t("Play"),
                        true,
                    )
                    .clicked()
                    {
                        out.push(UiAction::PlayTracks(tracks.clone(), 0));
                    }
                    if widgets::action_button(
                        ui,
                        egui_phosphor::regular::SHUFFLE,
                        crate::i18n::t("Shuffle"),
                        false,
                    )
                    .clicked()
                    {
                        out.push(UiAction::ShufflePlay(tracks.clone()));
                    }
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        common::track_rows(ui, &tracks, &ps, true, &mut out);
                        ui.add_space(24.0);
                    });
            }
        }
        LibraryTab::Albums => {
            let items: Vec<SearchItem> = app
                .local
                .albums
                .iter()
                .cloned()
                .map(SearchItem::Album)
                .collect();
            grid_or_message(
                ui,
                &items,
                crate::i18n::t("No saved albums"),
                "Use “Save” on an album page.",
                &mut out,
            );
        }
        LibraryTab::Artists => {
            let items: Vec<SearchItem> = app
                .local
                .artists
                .iter()
                .cloned()
                .map(SearchItem::Artist)
                .collect();
            grid_or_message(
                ui,
                &items,
                crate::i18n::t("No saved artists"),
                "Use “Save” on an artist page.",
                &mut out,
            );
        }
    }
    for a in out {
        app.run(a);
    }
}

pub fn playlists(ui: &mut Ui, app: &mut AppState) {
    let mut out = Vec::new();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t("Playlists"))
                .text_style(egui::TextStyle::Heading)
                .color(crate::theme::c_text()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(16.0);
            let split = widgets::split_button(
                ui,
                egui_phosphor::regular::PLUS,
                crate::i18n::t("New playlist"),
                false,
            );
            if split.main.clicked() {
                out.push(UiAction::NewPlaylistFor(Vec::new()));
            }
            let mut popup = egui::Popup::menu(&split.arrow);
            if crate::DEV_OPEN_MENU.load(std::sync::atomic::Ordering::Relaxed) {
                popup = popup.open(true);
            }
            popup.show(|ui| {
                super::common::style_menu(ui);
                if widgets::icon_menu_item(
                    ui,
                    egui_phosphor::regular::DOWNLOAD_SIMPLE,
                    crate::i18n::t("Import playlist"),
                    None,
                )
                .clicked()
                {
                    out.push(UiAction::ImportPlaylist);
                }
            });
        });
    });
    ui.add_space(12.0);
    let items = app
        .local
        .playlist_cards(&app.covers_dir().unwrap_or_default());
    grid_or_message(
        ui,
        &items,
        crate::i18n::t("No playlists yet"),
        "Create one with “New playlist”, or add songs from any song's right-click menu.",
        &mut out,
    );
    for a in out {
        app.run(a);
    }
}
