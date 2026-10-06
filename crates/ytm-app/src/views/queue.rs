use eframe::egui::{self, pos2, Context, Id, Rect, Ui};
use egui_phosphor::regular as icon;
use ytm_player::Command;

use crate::state::AppState;
use crate::theme::{self, c_text_dim};
use crate::widgets;

/// Width of the drawer, including its padding.
pub const DRAWER_WIDTH: f32 = 372.0;
const DRAWER_PADDING: i8 = 14;

/// Where the drawer is at animation progress `t` (0 = fully off-screen to the right, 1 = fully open).
/// It spans the area between the title bar and the player bar and overlaps the page.
pub fn drawer_rect(content: Rect, top_inset: f32, bottom_inset: f32, t: f32) -> Rect {
    let left = content.right() - DRAWER_WIDTH * t.clamp(0.0, 1.0);
    Rect::from_min_max(
        pos2(left, content.top() + top_inset),
        pos2(left + DRAWER_WIDTH, content.bottom() - bottom_inset),
    )
}

/// The queue as a floating drawer: slides in over the page (which does not move), on a lighter
/// surface with a soft shadow on its left edge so it reads as being above the content.
pub fn overlay(ctx: &Context, app: &mut AppState, top_inset: f32, bottom_inset: f32) {
    let t = ctx.animate_bool_with_time_and_easing(
        Id::new("queue_drawer_anim"),
        app.queue_open,
        0.22,
        egui::emath::easing::cubic_out,
    );
    if t <= 0.0 {
        return;
    }
    let rect = drawer_rect(ctx.content_rect(), top_inset, bottom_inset, t);
    egui::Area::new(Id::new("queue_drawer"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false) // it starts off-screen
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::c_elevated())
                .stroke(egui::Stroke::new(1.0, theme::c_border()))
                .corner_radius(egui::CornerRadius {
                    nw: 16,
                    sw: 16,
                    ne: 0,
                    se: 0,
                })
                .shadow(egui::epaint::Shadow {
                    offset: [-10, 0],
                    blur: 38,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(190),
                })
                .inner_margin(egui::Margin::symmetric(DRAWER_PADDING, 0))
                .show(ui, |ui| {
                    ui.set_width(rect.width() - 2.0 * f32::from(DRAWER_PADDING) - 2.0);
                    ui.set_height(rect.height() - 2.0);
                    show(ui, app);
                });
        });
}

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let ps = app.ps.clone();
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t("Queue"))
                .text_style(theme::title_style())
                .color(theme::c_text()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icon::X, 16.0, false, crate::i18n::t("Close queue"))
                .clicked()
            {
                app.queue_open = false;
            }
            if !ps.tracks.is_empty() && widgets::pill(ui, crate::i18n::t("Clear"), false).clicked()
            {
                app.playback(Command::Clear);
            }
            if !ps.tracks.is_empty()
                && widgets::pill(ui, crate::i18n::t("Save as playlist"), false).clicked()
            {
                app.run(crate::state::UiAction::NewPlaylistFor((*ps.tracks).clone()));
            }
        });
    });
    ui.add_space(6.0);
    if ps.tracks.is_empty() {
        ui.add_space(24.0);
        ui.label(
            egui::RichText::new(crate::i18n::t(
                "Your queue is empty. Play something to fill it.",
            ))
            .color(c_text_dim()),
        );
        return;
    }

    let playing = ps.status == ytm_player::Status::Playing;
    let mut jump = None;
    let mut remove = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if let Some(i) = ps.current {
                ui.label(
                    egui::RichText::new(crate::i18n::t("Now playing"))
                        .color(c_text_dim())
                        .size(12.5),
                );
                let r = widgets::track_row(ui, &ps.tracks[i], None, true, playing);
                r.context_menu(|ui| {
                    crate::views::common::style_menu(ui);
                    if ui.button(crate::i18n::t("Remove from queue")).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(crate::i18n::t("Next up"))
                        .color(c_text_dim())
                        .size(12.5),
                );
            }
            let upcoming = ps.upcoming.clone();
            for &idx in upcoming.iter() {
                let track = &ps.tracks[idx];
                let r = widgets::track_row(ui, track, None, false, false);
                if r.clicked() {
                    jump = Some(idx);
                }
                r.context_menu(|ui| {
                    crate::views::common::style_menu(ui);
                    if ui.button(crate::i18n::t("Play now")).clicked() {
                        jump = Some(idx);
                        ui.close();
                    }
                    if ui.button(crate::i18n::t("Remove from queue")).clicked() {
                        remove = Some(idx);
                        ui.close();
                    }
                });
            }
        });
    if let Some(i) = jump {
        app.playback(Command::Jump(i));
    }
    if let Some(i) = remove {
        app.playback(Command::Remove(i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn drawer_slides_from_off_screen_to_the_right_edge_between_the_bars() {
        let content = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 820.0));
        let closed = drawer_rect(content, 38.0, 88.0, 0.0);
        assert_eq!(closed.left(), content.right(), "fully hidden when closed");
        let open = drawer_rect(content, 38.0, 88.0, 1.0);
        assert_eq!(
            open.right(),
            content.right(),
            "flush with the right edge when open"
        );
        assert_eq!(open.width(), DRAWER_WIDTH);
        assert_eq!(
            (open.top(), open.bottom()),
            (38.0, 820.0 - 88.0),
            "leaves title bar and player bar alone"
        );
        let mid = drawer_rect(content, 38.0, 88.0, 0.5);
        assert!(mid.left() > open.left() && mid.left() < closed.left());
        assert_eq!(
            drawer_rect(content, 0.0, 0.0, 7.0),
            drawer_rect(content, 0.0, 0.0, 1.0),
            "progress is clamped"
        );
    }
}
