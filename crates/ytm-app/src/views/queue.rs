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
    let mut acts = Acts::default();
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
                        acts.remove = Some(i);
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
            upcoming_rows(ui, &ps.tracks, &ps.upcoming, &mut acts);
        });
    if let Some(i) = acts.jump {
        app.playback(Command::Jump(i));
    }
    if let Some(i) = acts.remove {
        app.playback(Command::Remove(i));
    }
    if let Some((from, to)) = acts.moved {
        app.playback(Command::MoveUpcoming { from, to });
    }
}

/// What the queue's rows asked for this frame; applied after drawing.
#[derive(Debug, Default, PartialEq, Eq)]
struct Acts {
    jump: Option<usize>,
    remove: Option<usize>,
    /// Positions within the "next up" list: (from, to).
    moved: Option<(usize, usize)>,
}

/// The "Next up" rows. Click plays one, the context menu removes it, and dragging a row moves it:
/// an insertion line shows where it will land and the move is only reported on release.
fn upcoming_rows(ui: &mut Ui, tracks: &[ytm_api::Track], upcoming: &[usize], acts: &mut Acts) {
    let n = upcoming.len();
    let key = Id::new("queue_drag");
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp::<usize>(key));
    let spacing = ui.spacing().item_spacing.y;
    let left = ui.cursor().left();
    let width = ui.available_width();
    let mut rects = Vec::with_capacity(n);
    let mut started = None;
    for (slot, &idx) in upcoming.iter().enumerate() {
        let track = &tracks[idx];
        let r =
            widgets::track_row_sense(ui, track, None, false, false, egui::Sense::click_and_drag());
        rects.push(r.rect);
        if r.drag_started() {
            started = Some(slot);
        }
        if dragging == Some(slot) {
            ui.painter().rect_filled(
                r.rect,
                egui::CornerRadius::same(10),
                theme::c_bg().gamma_multiply(0.67),
            );
        }
        if r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if r.clicked() {
            acts.jump = Some(idx);
        }
        r.context_menu(|ui| {
            crate::views::common::style_menu(ui);
            if ui.button(crate::i18n::t("Play now")).clicked() {
                acts.jump = Some(idx);
                ui.close();
            }
            if ui.button(crate::i18n::t("Remove from queue")).clicked() {
                acts.remove = Some(idx);
                ui.close();
            }
        });
    }
    if started.is_some() {
        dragging = started;
    }
    if let Some(from) = dragging {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        match pointer {
            Some(pos) if !cancelled && from < n => {
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
                    Id::new("queue_drag_layer"),
                ));
                if target.is_some() {
                    let y = gaps[gap];
                    painter.line_segment(
                        [pos2(left, y), pos2(left + width, y)],
                        egui::Stroke::new(3.0, theme::ACCENT),
                    );
                    painter.circle_filled(pos2(left, y), 5.0, theme::ACCENT);
                }
                let ghost = Rect::from_center_size(
                    pos2(left + width / 2.0, pos.y),
                    egui::vec2(width, widgets::ROW_HEIGHT - 8.0),
                );
                painter.rect_filled(
                    ghost,
                    egui::CornerRadius::same(10),
                    theme::c_surface_active(),
                );
                painter.text(
                    ghost.left_center() + egui::vec2(16.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &tracks[upcoming[from]].title,
                    egui::FontId::new(14.0, theme::bold_family()),
                    theme::c_text(),
                );
                let clip = ui.clip_rect();
                if pos.y < clip.top() + 48.0 {
                    ui.scroll_with_delta(egui::vec2(0.0, 14.0));
                } else if pos.y > clip.bottom() - 48.0 {
                    ui.scroll_with_delta(egui::vec2(0.0, -14.0));
                }
                ui.ctx().request_repaint();
                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(to) = target {
                        acts.moved = Some((from, to));
                    }
                    dragging = None;
                }
            }
            _ => dragging = None,
        }
    }
    ui.data_mut(|d| {
        match dragging {
            Some(v) => {
                d.insert_temp(key, v);
            }
            None => {
                d.remove_temp::<usize>(key);
            }
        };
    });
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

    // ---- dragging the "next up" rows, through real egui frames -----------------------------

    use eframe::egui::{Event, PointerButton, RawInput};

    const SCREEN: egui::Vec2 = egui::vec2(500.0, 700.0);

    struct Harness {
        ctx: Context,
        tracks: Vec<ytm_api::Track>,
        upcoming: Vec<usize>,
        top: f32,
        acts: Acts,
    }

    impl Harness {
        /// Track 0 is playing; tracks 1..n are "next up".
        fn new(n: usize) -> Self {
            let mut h = Self {
                ctx: Context::default(),
                tracks: (0..n)
                    .map(|i| ytm_api::Track {
                        video_id: format!("{i:011}"),
                        title: format!("t{i}"),
                        ..ytm_api::Track::default()
                    })
                    .collect(),
                upcoming: (1..n).collect(),
                top: 0.0,
                acts: Acts::default(),
            };
            crate::theme::install(&h.ctx);
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), SCREEN)),
                events,
                ..RawInput::default()
            };
            let (tracks, upcoming, top, acts) =
                (&self.tracks, &self.upcoming, &mut self.top, &mut self.acts);
            let mut out = self.ctx.run_ui(input, |ui| {
                *top = ui.cursor().top();
                upcoming_rows(ui, tracks, upcoming, acts);
            });
            out.textures_delta.clear();
        }

        fn pitch(&self) -> f32 {
            self.ctx.global_style().spacing.item_spacing.y + widgets::ROW_HEIGHT
        }

        /// Centre of the row in "next up" slot `i`.
        fn row(&self, i: usize) -> egui::Pos2 {
            pos2(
                SCREEN.x / 2.0,
                self.top + i as f32 * self.pitch() + widgets::ROW_HEIGHT / 2.0,
            )
        }

        /// The line between rows: gap `g` is just above slot `g`.
        fn gap(&self, g: usize) -> egui::Pos2 {
            pos2(SCREEN.x / 2.0, self.top + g as f32 * self.pitch())
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

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.press(from);
            self.drag_to(from, to);
            self.button(to, false);
            self.frame(vec![]);
        }
    }

    #[test]
    fn dragging_a_row_down_moves_it_only_on_release() {
        let mut h = Harness::new(5); // next up: t1 t2 t3 t4
        let (from, to) = (h.row(0), h.gap(3)); // t1 to just above t4
        h.press(from);
        h.drag_to(from, to);
        assert_eq!(h.acts, Acts::default(), "nothing is applied mid-drag");
        h.button(to, false);
        h.frame(vec![]);
        assert_eq!(h.acts.moved, Some((0, 2)), "{:?}", h.acts);
        assert_eq!(h.acts.jump, None, "a drag is not a click");
    }

    #[test]
    fn dragging_up_and_past_the_ends_clamps() {
        let mut h = Harness::new(5);
        let (from, to) = (h.row(3), h.gap(0));
        h.drag(from, to);
        assert_eq!(h.acts.moved, Some((3, 0)));

        let mut h = Harness::new(5);
        let (from, far_below) = (h.row(0), pos2(SCREEN.x / 2.0, h.top + 650.0));
        h.drag(from, far_below);
        assert_eq!(h.acts.moved, Some((0, 3)), "below the list means last");
    }

    #[test]
    fn a_plain_click_plays_that_song_and_dropping_in_place_does_nothing() {
        let mut h = Harness::new(5);
        let at = h.row(2);
        h.press(at);
        h.button(at, false);
        h.frame(vec![]);
        assert_eq!(h.acts.jump, Some(3), "slot 2 is track 3");
        assert_eq!(h.acts.moved, None);

        let mut h = Harness::new(5);
        let (from, nearby) = (h.row(1), h.row(1) + vec2(0.0, 8.0));
        h.drag(from, nearby);
        assert_eq!(h.acts.moved, None);
    }

    #[test]
    fn escape_cancels_a_drag() {
        let mut h = Harness::new(5);
        let (from, to) = (h.row(0), h.row(3));
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
        assert_eq!(h.acts.moved, None);
    }
}
