use std::time::Duration;

use eframe::egui::{
    self, pos2, vec2, Align2, CornerRadius, CursorIcon, FontId, Rect, Sense, Ui, UiBuilder,
};
use egui_phosphor::regular as icon;
use ytm_player::{Command, Repeat, Status};

use crate::state::AppState;
use crate::theme::{self, c_text, c_text_dim};
use crate::widgets;

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let full = ui.max_rect();
    let inner = full.shrink2(vec2(20.0, 0.0));
    let bar = crate::layout::player_bar(inner.width());

    let left = Rect::from_min_size(inner.min, vec2(bar.left, inner.height()));
    let right = Rect::from_min_max(pos2(inner.right() - bar.right, inner.top()), inner.max);
    let center = Rect::from_min_max(
        pos2(left.right() + 12.0, inner.top()),
        pos2(right.left() - 12.0, inner.bottom()),
    );

    ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
        now_playing_chip(ui, app, bar.show_text)
    });
    ui.scope_builder(UiBuilder::new().max_rect(center), |ui| {
        transport(ui, app, bar.show_shuffle_repeat)
    });
    ui.scope_builder(UiBuilder::new().max_rect(right), |ui| extras(ui, app, &bar));
}

fn now_playing_chip(ui: &mut Ui, app: &mut AppState, show_text: bool) {
    let rect = ui.max_rect();
    let Some(track) = app.ps.current_track().cloned() else {
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            crate::i18n::t("Nothing playing"),
            FontId::proportional(14.0),
            theme::c_text_faint(),
        );
        return;
    };
    let art = Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(56.0, 56.0));
    widgets::paint_art(
        ui,
        art,
        widgets::best_thumbnail(&track.thumbnails),
        CornerRadius::same(8),
    );
    if show_text {
        let x = art.right() + 14.0;
        let w = (rect.right() - x - 44.0).max(30.0);
        let p = ui.painter();
        p.galley(
            pos2(x, rect.center().y - 21.0),
            widgets::fit_text(
                p,
                &track.title,
                FontId::new(14.5, theme::bold_family()),
                c_text(),
                w,
            ),
            c_text(),
        );
        p.galley(
            pos2(x, rect.center().y + 2.0),
            widgets::fit_text(
                p,
                &track.artist_line(),
                FontId::proportional(13.0),
                c_text_dim(),
                w,
            ),
            c_text_dim(),
        );
    }
    let heart =
        Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(40.0, 40.0));
    let liked = app.is_liked(&track.video_id);
    ui.scope_builder(UiBuilder::new().max_rect(heart), |ui| {
        let tip = if liked {
            crate::i18n::t("Remove from liked songs")
        } else {
            crate::i18n::t("Add to liked songs")
        };
        let clicked = if liked {
            widgets::icon_button_filled(ui, icon::HEART, 18.0, true, tip).clicked()
        } else {
            widgets::icon_button(ui, icon::HEART, 18.0, false, tip).clicked()
        };
        if clicked {
            app.toggle_like(&track);
        }
    });
    let hit = Rect::from_min_max(rect.min, pos2(rect.right() - 44.0, rect.bottom()));
    let resp = ui.interact(hit, ui.id().with("chip"), Sense::click());
    if resp.clicked() {
        app.now_playing_open = !app.now_playing_open;
    }
    resp.on_hover_cursor(CursorIcon::PointingHand);
}

fn transport(ui: &mut Ui, app: &mut AppState, shuffle_repeat: bool) {
    let rect = ui.max_rect();
    let ps = app.ps.clone();
    let has_track = ps.current.is_some();
    let loading = ps.status == Status::Loading;
    let playing = ps.status == Status::Playing;

    // Buttons row, centred.
    let buttons = if shuffle_repeat { 4.0 } else { 2.0 };
    let row_w = 38.0 * buttons + 44.0 + 10.0 * buttons;
    let row_rect = Rect::from_min_size(
        pos2(rect.center().x - row_w / 2.0, rect.top() + 10.0),
        vec2(row_w, 44.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(row_rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if shuffle_repeat
                && widgets::icon_button(
                    ui,
                    icon::SHUFFLE,
                    20.0,
                    ps.shuffle,
                    crate::i18n::t("Shuffle"),
                )
                .clicked()
            {
                app.playback(Command::SetShuffle(!ps.shuffle));
            }
            if widgets::icon_button(ui, icon::SKIP_BACK, 22.0, false, crate::i18n::t("Previous"))
                .clicked()
            {
                app.playback(Command::Prev);
            }
            if widgets::play_button(ui, playing, loading, 44.0).clicked() {
                app.playback(Command::Toggle);
            }
            if widgets::icon_button(ui, icon::SKIP_FORWARD, 22.0, false, crate::i18n::t("Next"))
                .clicked()
            {
                app.playback(Command::Next);
            }
            let (glyph, tip) = match ps.repeat {
                Repeat::One => (icon::REPEAT_ONCE, crate::i18n::t("Repeat one")),
                Repeat::All => (icon::REPEAT, crate::i18n::t("Repeat all")),
                Repeat::Off => (icon::REPEAT, crate::i18n::t("Repeat off")),
            };
            if shuffle_repeat
                && widgets::icon_button(ui, glyph, 20.0, ps.repeat != Repeat::Off, tip).clicked()
            {
                app.playback(Command::SetRepeat(ps.repeat.cycle()));
            }
        });
    });

    // Seek row.
    let bar_w = rect.width().min(640.0);
    let seek_rect = Rect::from_min_size(
        pos2(rect.center().x - bar_w / 2.0, rect.top() + 58.0),
        vec2(bar_w, 20.0),
    );
    let pos = if has_track {
        app.player.position()
    } else {
        Duration::ZERO
    };
    let dur = ps.duration.unwrap_or_default();
    let fraction = if dur.is_zero() {
        0.0
    } else {
        (pos.as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
    };
    ui.scope_builder(UiBuilder::new().max_rect(seek_rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let time = |ui: &mut Ui, secs: u64, align_right: bool| {
                let (r, _) = ui.allocate_exact_size(vec2(44.0, 18.0), Sense::hover());
                let (anchor, x) = if align_right {
                    (Align2::RIGHT_CENTER, r.right())
                } else {
                    (Align2::LEFT_CENTER, r.left())
                };
                ui.painter().text(
                    pos2(x, r.center().y),
                    anchor,
                    widgets::format_time(secs),
                    FontId::proportional(12.0),
                    c_text_dim(),
                );
            };
            time(ui, pos.as_secs(), true);
            let w = ui.available_width() - 44.0 - 10.0;
            if let Some(frac) = widgets::seek_bar(ui, fraction, w, has_track && !dur.is_zero()) {
                app.playback(Command::Seek(Duration::from_secs_f32(
                    frac * dur.as_secs_f32(),
                )));
            }
            time(ui, dur.as_secs(), false);
        });
    });
}

fn extras(ui: &mut Ui, app: &mut AppState, bar: &crate::layout::PlayerBar) {
    let ps = app.ps.clone();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (glyph, tip) = if app.now_playing_open {
            (icon::CARET_DOWN, crate::i18n::t("Close now playing"))
        } else {
            (icon::CARET_UP, crate::i18n::t("Open now playing"))
        };
        if widgets::icon_button(ui, glyph, 20.0, false, tip).clicked() {
            app.now_playing_open = !app.now_playing_open;
        }
        if widgets::icon_button(
            ui,
            icon::QUEUE,
            20.0,
            app.queue_open,
            crate::i18n::t("Queue"),
        )
        .clicked()
        {
            app.queue_open = !app.queue_open;
        }
        ui.add_space(6.0);
        // Volume (right-to-left: slider first, then its icon to the left). The narrowest bar has
        // room for neither; M and Ctrl+Up/Down still work.
        if bar.show_volume {
            if let Some(f) = widgets::seek_bar(ui, ps.volume, 96.0, true) {
                app.muted_volume = None;
                app.playback(Command::SetVolume(f));
            }
        }
        if bar.right >= 130.0 {
            let vol_icon = match ps.volume {
                v if v <= 0.001 => icon::SPEAKER_X,
                v if v < 0.5 => icon::SPEAKER_LOW,
                _ => icon::SPEAKER_HIGH,
            };
            if widgets::icon_button(ui, vol_icon, 20.0, false, crate::i18n::t("Mute")).clicked() {
                app.toggle_mute();
            }
        }
    });
}
