use eframe::egui::{self, pos2, vec2, CornerRadius, Id, Sense, Ui};
use egui_phosphor::regular as icon;

use crate::state::{AppState, Load, Route, UiAction};
use crate::theme::{self, c_text, c_text_dim};
use crate::views::common;
use crate::widgets;
use ytm_api::{PlaylistSummary, SearchItem};

pub fn show(ui: &mut Ui, app: &mut AppState, id: &str) {
    if crate::local::is_local_id(id) {
        return local(ui, app, id);
    }
    match app.playlists.get(id).cloned().unwrap_or_default() {
        Load::Idle | Load::Loading => common::centered_spinner(ui),
        Load::Failed(e) => {
            if common::error_panel(ui, crate::i18n::t("Could not load this playlist"), &e) {
                app.reload();
            }
        }
        Load::Ready(page) => {
            let mut out = Vec::new();
            let mut want_more = false;
            let ps = app.ps.clone();
            let saved = app.is_saved(&SearchItem::Playlist(PlaylistSummary {
                playlist_id: page.playlist_id.clone(),
                ..PlaylistSummary::default()
            }));
            let loading_more = app.playlist_more_loading;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 28.0;
                        let (rect, _) = ui.allocate_exact_size(vec2(220.0, 220.0), Sense::hover());
                        widgets::paint_art(
                            ui,
                            rect,
                            widgets::best_thumbnail(&page.thumbnails),
                            CornerRadius::same(theme::CARD_RADIUS),
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            ui.label(
                                egui::RichText::new(crate::i18n::t("Playlist"))
                                    .color(c_text_dim())
                                    .size(13.0),
                            );
                            ui.label(theme::bold(&page.title).size(34.0).color(c_text()));
                            if !page.author.is_empty() {
                                ui.label(theme::bold(&page.author).size(15.0).color(c_text()));
                            }
                            ui.label(
                                egui::RichText::new(&page.stats)
                                    .color(c_text_dim())
                                    .size(13.0),
                            );
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                if widgets::action_button(
                                    ui,
                                    icon::PLAY,
                                    crate::i18n::t("Play"),
                                    true,
                                )
                                .clicked()
                                {
                                    out.push(UiAction::PlayTracks(page.tracks.clone(), 0));
                                }
                                if widgets::action_button(
                                    ui,
                                    icon::SHUFFLE,
                                    crate::i18n::t("Shuffle"),
                                    false,
                                )
                                .clicked()
                                {
                                    out.push(UiAction::ShufflePlay(page.tracks.clone()));
                                }
                                if widgets::action_button(
                                    ui,
                                    icon::QUEUE,
                                    crate::i18n::t("Add to queue"),
                                    false,
                                )
                                .clicked()
                                {
                                    out.push(UiAction::Enqueue(page.tracks.clone()));
                                }
                                common::save_button(
                                    ui,
                                    saved,
                                    SearchItem::Playlist(PlaylistSummary {
                                        playlist_id: page.playlist_id.clone(),
                                        title: page.title.clone(),
                                        subtitle: page.author.clone(),
                                        thumbnails: page.thumbnails.clone(),
                                        covers: Vec::new(),
                                    }),
                                    &mut out,
                                );
                            });
                        });
                    });
                    ui.add_space(18.0);
                    common::track_rows(ui, &page.tracks, &ps, true, &mut out);
                    if page.continuation.is_some() {
                        // Sentinel: when it scrolls into view, fetch the next page.
                        let (rect, _) = ui.allocate_exact_size(vec2(1.0, 40.0), Sense::hover());
                        if ui.is_rect_visible(rect) && !loading_more {
                            want_more = true;
                        }
                        if loading_more {
                            ui.vertical_centered(|ui| {
                                ui.add(egui::Spinner::new().color(theme::ACCENT));
                            });
                        }
                    }
                    ui.add_space(24.0);
                });
            if want_more {
                app.load_more_playlist(id);
            }
            for a in out {
                app.run(a);
            }
        }
    }
}

/// A playlist stored on this device.
fn local(ui: &mut Ui, app: &mut AppState, id: &str) {
    let Some(playlist) = app.local.playlist(id).cloned() else {
        common::message(
            ui,
            crate::i18n::t("Playlist not found"),
            crate::i18n::t("It may have been deleted."),
        );
        if widgets::pill(ui, crate::i18n::t("Back to playlists"), true).clicked() {
            app.navigate(Route::Playlists);
        }
        return;
    };
    let ps = app.ps.clone();
    let editing = app.playlist_edit;
    let mut out: Vec<UiAction> = Vec::new();
    let current = ps.current_track().map(|t| t.video_id.clone());
    let playing = ps.status == ytm_player::Status::Playing;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 28.0;
                let (rect, _) = ui.allocate_exact_size(vec2(220.0, 220.0), Sense::hover());
                let custom = app.covers_dir().and_then(|dir| playlist.cover_uri(&dir));
                let covers = playlist.covers();
                if custom.is_some() {
                    widgets::paint_art(
                        ui,
                        rect,
                        custom.as_deref(),
                        CornerRadius::same(theme::CARD_RADIUS),
                    );
                } else if covers.is_empty() {
                    let art = playlist
                        .tracks
                        .iter()
                        .find_map(|t| widgets::best_thumbnail(&t.thumbnails));
                    widgets::paint_art(ui, rect, art, CornerRadius::same(theme::CARD_RADIUS));
                } else {
                    widgets::paint_mosaic(
                        ui,
                        rect,
                        &covers,
                        CornerRadius::same(theme::CARD_RADIUS),
                    );
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(
                        egui::RichText::new(crate::i18n::t("Playlist on this device"))
                            .color(c_text_dim())
                            .size(13.0),
                    );
                    ui.label(theme::bold(&playlist.title).size(34.0).color(c_text()));
                    let n = playlist.song_count();
                    ui.label(
                        egui::RichText::new(format!("{n} song{}", if n == 1 { "" } else { "s" }))
                            .color(c_text_dim())
                            .size(13.0),
                    );
                    ui.add_space(6.0);
                    header_buttons(ui, &playlist, editing, &mut out);
                });
            });
            ui.add_space(18.0);
            if playlist.pending() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.spinner());
            } else if playlist.tracks.is_empty() {
                ui.label(
                    egui::RichText::new("Empty. Right-click any song → Add to playlist.")
                        .color(c_text_dim()),
                );
            }
            if editing {
                edit_rows(ui, &playlist, &mut out);
            } else {
                let last = playlist.tracks.len().saturating_sub(1);
                for (i, t) in playlist.tracks.iter().enumerate() {
                    let is_current = current.as_deref() == Some(t.video_id.as_str());
                    let r = widgets::track_row(ui, t, Some(i), is_current, playing);
                    if r.clicked() {
                        out.push(UiAction::PlayTracks(playlist.tracks.clone(), i));
                    }
                    let pid = playlist.id.clone();
                    common::track_menu_with(&r, t, &mut out, |ui, out| {
                        if ui
                            .button(crate::i18n::t("Remove from this playlist"))
                            .clicked()
                        {
                            out.push(UiAction::LocalRemoveTrack {
                                playlist_id: pid.clone(),
                                index: i,
                            });
                        }
                        if i > 0 && ui.button(crate::i18n::t("Move up")).clicked() {
                            out.push(UiAction::LocalMoveTrack {
                                playlist_id: pid.clone(),
                                index: i,
                                delta: -1,
                            });
                        }
                        if i < last && ui.button(crate::i18n::t("Move down")).clicked() {
                            out.push(UiAction::LocalMoveTrack {
                                playlist_id: pid.clone(),
                                index: i,
                                delta: 1,
                            });
                        }
                        ui.separator();
                    });
                }
            }
            ui.add_space(24.0);
        });
    for a in out {
        app.run(a);
    }
}

/// Edit mode: drag the handle to reorder (a line shows where the song will land), X removes a song.
fn edit_rows(ui: &mut Ui, playlist: &crate::local::LocalPlaylist, out: &mut Vec<UiAction>) {
    let n = playlist.tracks.len();
    let key = Id::new(("playlist_drag", &playlist.id));
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp::<usize>(key));
    let spacing = ui.spacing().item_spacing.y;
    let left = ui.cursor().left();
    let width = ui.available_width();

    let mut remove = None;
    let mut started = None;
    let mut rects = Vec::with_capacity(n);
    for (i, t) in playlist.tracks.iter().enumerate() {
        let r = widgets::edit_row(ui, t, i, dragging == Some(i));
        rects.push(r.rect);
        if r.remove.clicked() {
            remove = Some(i);
        }
        if r.handle.drag_started() {
            started = Some(i);
        }
    }
    if started.is_some() {
        dragging = started;
    }

    if let Some(from) = dragging {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        match pointer {
            Some(pos) if !cancelled => {
                // gap lines measured from the rows laid out this frame, so they stay on the
                // gaps while the list scrolls
                let gaps: Vec<f32> = (0..=n)
                    .map(|g| match rects.get(g) {
                        Some(r) => r.top() - spacing / 2.0,
                        None => rects.last().map_or(0.0, |r| r.bottom() + spacing / 2.0),
                    })
                    .collect();
                let gap = widgets::drop_gap(pos.y, &gaps);
                let target = widgets::drop_target(from, gap);
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    Id::new("playlist_drag_layer"),
                ));
                if target.is_some() {
                    // the line sits in the gap between rows where the song will land
                    let y = gaps[gap];
                    painter.line_segment(
                        [pos2(left, y), pos2(left + width, y)],
                        egui::Stroke::new(3.0, theme::ACCENT),
                    );
                    painter.circle_filled(pos2(left, y), 5.0, theme::ACCENT);
                }
                // floating copy of the dragged song under the pointer
                let ghost = egui::Rect::from_center_size(
                    pos2(left + width / 2.0, pos.y),
                    vec2(width.min(520.0), widgets::ROW_HEIGHT - 8.0),
                );
                painter.rect_filled(ghost, CornerRadius::same(10), theme::c_surface_active());
                painter.text(
                    ghost.left_center() + vec2(16.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &playlist.tracks[from].title,
                    egui::FontId::new(14.0, theme::bold_family()),
                    c_text(),
                );
                // keep long lists reachable while dragging
                let clip = ui.clip_rect();
                if pos.y < clip.top() + 48.0 {
                    ui.scroll_with_delta(vec2(0.0, 14.0));
                } else if pos.y > clip.bottom() - 48.0 {
                    ui.scroll_with_delta(vec2(0.0, -14.0));
                }
                ui.ctx().request_repaint();

                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(to) = target {
                        out.push(UiAction::LocalMoveTrack {
                            playlist_id: playlist.id.clone(),
                            index: from,
                            delta: to as isize - from as isize,
                        });
                    }
                    dragging = None;
                }
            }
            _ => dragging = None, // Esc, or the pointer left the window
        }
    }
    ui.data_mut(|d| match dragging {
        Some(i) => {
            d.insert_temp(key, i);
        }
        None => {
            d.remove_temp::<usize>(key);
        }
    });

    if let Some(i) = remove {
        out.push(UiAction::LocalRemoveTrack {
            playlist_id: playlist.id.clone(),
            index: i,
        });
    }
    if n == 0 {
        ui.label(egui::RichText::new(crate::i18n::t("Nothing to edit yet.")).color(c_text_dim()));
    }
}

/// Play / Shuffle / Add to queue, and the Edit split button with its Rename / Delete dropdown.
fn header_buttons(
    ui: &mut Ui,
    playlist: &crate::local::LocalPlaylist,
    editing: bool,
    out: &mut Vec<UiAction>,
) -> widgets::SplitButton {
    ui.horizontal(|ui| {
        if widgets::action_button(ui, icon::PLAY, crate::i18n::t("Play"), true).clicked() {
            out.push(UiAction::PlayTracks(playlist.tracks.clone(), 0));
        }
        if widgets::action_button(ui, icon::SHUFFLE, crate::i18n::t("Shuffle"), false).clicked() {
            out.push(UiAction::ShufflePlay(playlist.tracks.clone()));
        }
        if widgets::action_button(ui, icon::QUEUE, crate::i18n::t("Add to queue"), false).clicked()
        {
            out.push(UiAction::Enqueue(playlist.tracks.clone()));
        }
        let (glyph, label) = if editing {
            (icon::CHECK, crate::i18n::t("Done"))
        } else {
            (icon::ARROWS_DOWN_UP, crate::i18n::t("Edit"))
        };
        let split = widgets::split_button(ui, glyph, label, editing);
        if split.main.clicked() {
            out.push(UiAction::SetPlaylistEdit(!editing));
        }
        let mut popup = egui::Popup::menu(&split.arrow);
        if crate::DEV_OPEN_MENU.load(std::sync::atomic::Ordering::Relaxed) {
            popup = popup.open(true);
        }
        popup.show(|ui| {
            common::style_menu(ui);
            if widgets::icon_menu_item(ui, icon::PENCIL_SIMPLE, crate::i18n::t("Rename"), None)
                .clicked()
            {
                out.push(UiAction::RenamePlaylist(playlist.id.clone()));
            }
            if widgets::icon_menu_item(ui, icon::IMAGE, crate::i18n::t("Change cover"), None)
                .clicked()
            {
                out.push(UiAction::ChangePlaylistCover(playlist.id.clone()));
            }
            if playlist.cover.is_some()
                && widgets::icon_menu_item(ui, icon::IMAGE, crate::i18n::t("Remove cover"), None)
                    .clicked()
            {
                out.push(UiAction::RemovePlaylistCover(playlist.id.clone()));
            }
            if widgets::icon_menu_item(ui, icon::EXPORT, crate::i18n::t("Export"), None).clicked() {
                out.push(UiAction::ExportPlaylist(playlist.id.clone()));
            }
            if widgets::icon_menu_item(
                ui,
                icon::TRASH,
                crate::i18n::t("Delete playlist"),
                Some(theme::ACCENT_HOVER),
            )
            .clicked()
            {
                out.push(UiAction::DeletePlaylist(playlist.id.clone()));
            }
        });
        split
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalPlaylist;
    use eframe::egui::{Event, PointerButton, RawInput};
    use ytm_api::Track;

    fn playlist(n: usize) -> LocalPlaylist {
        LocalPlaylist {
            cover: None,
            stub: None,
            id: "local:1".into(),
            title: "P".into(),
            tracks: (0..n)
                .map(|i| Track {
                    video_id: format!("{i:011}"),
                    title: format!("song {i}"),
                    ..Track::default()
                })
                .collect(),
        }
    }

    const SCREEN: egui::Vec2 = egui::vec2(900.0, 700.0);
    const MARGIN: f32 = 0.0; // the root Ui has no margin

    /// Drives `edit_rows` through real egui frames with synthetic pointer input.
    struct Harness {
        ctx: egui::Context,
        pl: LocalPlaylist,
        top: f32,
        actions: Vec<UiAction>,
    }

    impl Harness {
        fn new(n: usize) -> Self {
            let mut h = Self {
                ctx: egui::Context::default(),
                pl: playlist(n),
                top: 0.0,
                actions: Vec::new(),
            };
            crate::theme::install(&h.ctx);
            h.frame(vec![]); // first layout, so `top` is known
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), SCREEN)),
                events,
                ..RawInput::default()
            };
            let (pl, top, actions) = (&self.pl, &mut self.top, &mut self.actions);
            let mut output = self.ctx.run_ui(input, |ui| {
                *top = ui.cursor().top();
                edit_rows(ui, pl, actions);
            });
            output.textures_delta.clear(); // no GPU here
        }

        fn pitch(&self) -> f32 {
            self.ctx.global_style().spacing.item_spacing.y + widgets::ROW_HEIGHT
        }

        fn row_y(&self, i: usize) -> f32 {
            self.top + i as f32 * self.pitch() + widgets::ROW_HEIGHT / 2.0
        }

        /// The line between rows: gap `g` is just above row `g`.
        fn gap(&self, g: usize) -> egui::Pos2 {
            pos2(MARGIN + 2.0 + 20.0, self.top + g as f32 * self.pitch())
        }

        /// Centre of row `i`'s drag handle.
        fn handle(&self, i: usize) -> egui::Pos2 {
            pos2(MARGIN + 2.0 + 20.0, self.row_y(i))
        }

        /// Centre of row `i`'s remove button.
        fn remove_button(&self, i: usize) -> egui::Pos2 {
            pos2(SCREEN.x - MARGIN - 28.0, self.row_y(i))
        }

        fn button(&mut self, at: egui::Pos2, pressed: bool) {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }]);
        }

        fn press(&mut self, at: egui::Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.button(at, true);
        }

        fn drag_to(&mut self, from: egui::Pos2, to: egui::Pos2) {
            for step in 1..=6 {
                let t = step as f32 / 6.0;
                self.frame(vec![Event::PointerMoved(from + (to - from) * t)]);
            }
        }

        /// press on `from`, drag to `to`, release.
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.press(from);
            self.drag_to(from, to);
            self.button(to, false);
            self.frame(vec![]);
        }
    }

    fn moves(actions: &[UiAction]) -> Vec<(usize, isize)> {
        actions
            .iter()
            .filter_map(|a| match a {
                UiAction::LocalMoveTrack { index, delta, .. } => Some((*index, *delta)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dragging_a_handle_down_reorders_only_on_release() {
        let mut h = Harness::new(5);
        let (from, to) = (h.handle(1), h.gap(4)); // below row 3
        h.press(from);
        h.drag_to(from, to);
        assert!(moves(&h.actions).is_empty(), "nothing is applied mid-drag");
        h.button(to, false);
        h.frame(vec![]);
        assert_eq!(moves(&h.actions), vec![(1, 2)], "{:?}", h.actions);
    }

    #[test]
    fn dragging_up_and_past_the_ends_clamps() {
        let mut h = Harness::new(5);
        let (from, to) = (h.handle(4), h.gap(0));
        h.drag(from, to);
        assert_eq!(moves(&h.actions), vec![(4, -4)]);

        let mut h = Harness::new(5);
        let (from, far_below) = (h.handle(0), pos2(32.0, h.top + 600.0));
        h.drag(from, far_below);
        assert_eq!(moves(&h.actions), vec![(0, 4)], "below the list means last");

        let mut h = Harness::new(5);
        let (from, far_above) = (h.handle(3), pos2(32.0, 2.0));
        h.drag(from, far_above);
        assert_eq!(
            moves(&h.actions),
            vec![(3, -3)],
            "above the list means first"
        );
    }

    #[test]
    fn dropping_where_it_started_changes_nothing() {
        let mut h = Harness::new(4);
        let (from, nearby) = (h.handle(2), h.handle(2) + vec2(0.0, 8.0));
        h.drag(from, nearby);
        assert!(moves(&h.actions).is_empty(), "{:?}", h.actions);
    }

    #[test]
    fn escape_cancels_a_drag() {
        let mut h = Harness::new(4);
        let (from, to) = (h.handle(0), h.handle(3));
        h.press(from);
        h.drag_to(from, to);
        h.frame(vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        h.button(to, false);
        h.frame(vec![]);
        assert!(moves(&h.actions).is_empty(), "{:?}", h.actions);
    }

    #[test]
    fn remove_button_removes_that_song_only() {
        let mut h = Harness::new(4);
        let at = h.remove_button(2);
        h.press(at);
        h.button(at, false);
        h.frame(vec![]);
        let removed: Vec<usize> = h
            .actions
            .iter()
            .filter_map(|a| match a {
                UiAction::LocalRemoveTrack { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(removed, vec![2], "{:?}", h.actions);
        assert!(moves(&h.actions).is_empty());
    }

    #[test]
    fn clicking_a_row_body_does_not_play_or_move_in_edit_mode() {
        let mut h = Harness::new(3);
        let body = pos2(300.0, h.row_y(1));
        h.press(body);
        h.button(body, false);
        h.frame(vec![]);
        assert!(h.actions.is_empty(), "{:?}", h.actions);
    }

    /// Runs `header_buttons` and finds the Edit split button's two halves.
    struct HeaderHarness {
        ctx: egui::Context,
        pl: LocalPlaylist,
        editing: bool,
        rects: Option<(egui::Rect, egui::Rect)>,
        actions: Vec<UiAction>,
    }

    impl HeaderHarness {
        fn new(editing: bool) -> Self {
            let mut h = Self {
                ctx: egui::Context::default(),
                pl: playlist(3),
                editing,
                rects: None,
                actions: Vec::new(),
            };
            crate::theme::install(&h.ctx);
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), SCREEN)),
                events,
                ..RawInput::default()
            };
            let (pl, editing, rects, actions) =
                (&self.pl, self.editing, &mut self.rects, &mut self.actions);
            let mut output = self.ctx.run_ui(input, |ui| {
                let split = header_buttons(ui, pl, editing, actions);
                *rects = Some((split.main.rect, split.arrow.rect));
            });
            output.textures_delta.clear();
        }

        fn part(&self, which: &str) -> egui::Rect {
            let (main, arrow) = self.rects.expect("header was laid out");
            if which == "main" {
                main
            } else {
                arrow
            }
        }

        fn click(&mut self, at: egui::Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }]);
            }
            self.frame(vec![]);
        }

        fn edit_toggles(&self) -> Vec<bool> {
            self.actions
                .iter()
                .filter_map(|a| match a {
                    UiAction::SetPlaylistEdit(on) => Some(*on),
                    _ => None,
                })
                .collect()
        }
    }

    #[test]
    fn edit_main_part_toggles_edit_mode_without_opening_the_dropdown() {
        let mut h = HeaderHarness::new(false);
        let at = h.part("main").center();
        h.click(at);
        assert_eq!(h.edit_toggles(), vec![true], "{:?}", h.actions);
        assert!(!egui::Popup::is_any_open(&h.ctx), "dropdown stays closed");
    }

    #[test]
    fn done_leaves_edit_mode() {
        let mut h = HeaderHarness::new(true);
        let at = h.part("main").center();
        h.click(at);
        assert_eq!(h.edit_toggles(), vec![false]);
    }

    #[test]
    fn arrow_part_opens_the_dropdown_and_does_not_toggle_edit() {
        let mut h = HeaderHarness::new(false);
        let (main, arrow) = (h.part("main"), h.part("arrow"));
        assert!(
            main.right() <= arrow.left() + 0.5,
            "halves sit side by side"
        );
        assert!(
            main.width() > arrow.width(),
            "the label part is the big one"
        );
        let at = arrow.center();
        h.click(at);
        assert!(h.edit_toggles().is_empty(), "{:?}", h.actions);
        assert!(egui::Popup::is_any_open(&h.ctx), "dropdown is open");
        // and it still works while in edit mode
        let mut h = HeaderHarness::new(true);
        let at = h.part("arrow").center();
        h.click(at);
        assert!(h.edit_toggles().is_empty() && egui::Popup::is_any_open(&h.ctx));
    }
}
