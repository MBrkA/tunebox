use eframe::egui::{self, Ui};
use ytm_api::{SearchFilter, SearchItem, Track};

use crate::state::{AppState, UiAction};
use crate::theme;
use crate::views::common;
use crate::widgets::{self, ROW_HEIGHT};

const FILTERS: [(SearchFilter, &str); 6] = [
    (SearchFilter::All, "All"),
    (SearchFilter::Songs, "Songs"),
    (SearchFilter::Videos, "Videos"),
    (SearchFilter::Albums, "Albums"),
    (SearchFilter::Artists, "Artists"),
    (SearchFilter::Playlists, "Playlists"),
];

pub fn show(ui: &mut Ui, app: &mut AppState) {
    ui.add_space(8.0);
    // Wraps onto a second line in narrow windows instead of running off the edge.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for (filter, label) in FILTERS {
            if widgets::pill(ui, crate::i18n::t(label), app.search.filter == filter).clicked() {
                app.set_filter(filter);
            }
        }
    });
    ui.add_space(8.0);

    if app.search.loading {
        common::centered_spinner(ui);
        return;
    }
    if let Some(err) = app.search.error.clone() {
        if common::error_panel(ui, crate::i18n::t("Search failed"), &err) {
            let q = app.search.query.clone();
            app.submit_search(&q);
        }
        return;
    }
    if app.search.items.is_empty() {
        common::message(
            ui,
            crate::i18n::t("No results"),
            &format!("Nothing found for “{}”.", app.search.query),
        );
        return;
    }

    let mut out: Vec<UiAction> = Vec::new();
    match app.search.filter {
        SearchFilter::Songs | SearchFilter::Videos => track_list(ui, app, &mut out),
        SearchFilter::Albums | SearchFilter::Artists | SearchFilter::Playlists => {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let items = app.search.items.clone();
                    let refs: Vec<&SearchItem> = items.iter().collect();
                    grid(ui, &refs, &mut out);
                    load_more_button(ui, app);
                });
        }
        SearchFilter::All => mixed(ui, app, &mut out),
    }
    for a in out {
        app.run(a);
    }
}

/// Songs/videos: a virtualised list with infinite scroll.
fn track_list(ui: &mut Ui, app: &mut AppState, out: &mut Vec<UiAction>) {
    let tracks = app.search.tracks();
    let n = tracks.len();
    let ps = app.ps.clone();
    let current_id = ps.current_track().map(|t| t.video_id.clone());
    let playing = ps.status == ytm_player::Status::Playing;
    let has_more = app.search.continuation.is_some();
    let mut want_more = false;
    let row_h = ROW_HEIGHT + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show_rows(ui, row_h, n, |ui, range| {
            want_more = range.end + 4 >= n && has_more;
            for i in range {
                let t: &Track = &tracks[i];
                let is_current = current_id.as_deref() == Some(t.video_id.as_str());
                let r = widgets::track_row(ui, t, Some(i), is_current, playing);
                if r.clicked() {
                    out.push(UiAction::PlayTracks((*tracks).clone(), i));
                }
                common::track_menu(&r, t, out);
            }
        });
    if want_more {
        app.load_more_results();
    }
}

/// crate::i18n::t("All"): songs first, then the other kinds in sections.
fn mixed(ui: &mut Ui, app: &mut AppState, out: &mut Vec<UiAction>) {
    let tracks = app.search.tracks();
    let ps = app.ps.clone();
    let items = app.search.items.clone();
    let of_kind = |f: fn(&SearchItem) -> bool| -> Vec<&SearchItem> {
        items.iter().filter(|i| f(i)).collect()
    };
    let sections = [
        (
            crate::i18n::t("Artists"),
            of_kind(|i| matches!(i, SearchItem::Artist(_))),
        ),
        (
            crate::i18n::t("Albums"),
            of_kind(|i| matches!(i, SearchItem::Album(_))),
        ),
        (
            crate::i18n::t("Playlists"),
            of_kind(|i| matches!(i, SearchItem::Playlist(_))),
        ),
    ];
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if !tracks.is_empty() {
                widgets::section_title(ui, crate::i18n::t("Songs"));
                common::track_rows(ui, &tracks, &ps, false, out);
            }
            for (title, section) in &sections {
                if section.is_empty() {
                    continue;
                }
                widgets::section_title(ui, title);
                grid(ui, section, out);
            }
            load_more_button(ui, app);
            ui.add_space(24.0);
        });
}

/// A wrapped grid of cards.
fn grid(ui: &mut Ui, items: &[&SearchItem], out: &mut Vec<UiAction>) {
    let gap = 14.0;
    let (cols, w) = widgets::grid_layout(ui.available_width(), 184.0, gap);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    for chunk in items.chunks(cols) {
        ui.horizontal_top(|ui| {
            for item in chunk {
                let _ = common::item_card(ui, item, w, &[], out);
            }
        });
    }
}

fn load_more_button(ui: &mut Ui, app: &mut AppState) {
    if app.search.continuation.is_none() {
        return;
    }
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        if app.search.loading_more {
            ui.add(egui::Spinner::new().color(theme::ACCENT));
        } else if widgets::pill(ui, crate::i18n::t("Load more"), false).clicked() {
            app.load_more_results();
        }
    });
}
