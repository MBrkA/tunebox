//! Listening history: what was played, how much, and the favourites. Everything is computed from
//! the history file on this device; nothing is sent anywhere.

use std::sync::Arc;

use eframe::egui::{self, pos2, vec2, Align2, CornerRadius, FontId, Rect, Sense, Ui};
use egui_phosphor::regular as icon;
use ytm_api::Track;

use crate::history::{Period, Stats};
use crate::state::{AppState, Route, UiAction};
use crate::theme::{self, c_text, c_text_dim, ACCENT};
use crate::views::common;
use crate::widgets;

/// Songs in the "Recently played" list.
const RECENT: usize = 50;
const TOP: usize = 10;

/// What one frame needs, computed once per change of period or history length.
struct Page {
    stats: Stats,
    recent: Vec<Track>,
}

/// "3 h 20 min", "45 min", "< 1 min".
pub fn duration_label(secs: u64) -> String {
    let minutes = secs / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => format!("< 1 {}", crate::i18n::t("min")),
        (0, m) => format!("{m} {}", crate::i18n::t("min")),
        (h, m) => format!("{h} {} {m} {}", crate::i18n::t("h"), crate::i18n::t("min")),
    }
}

fn page(ui: &Ui, app: &AppState) -> Arc<Page> {
    let key = egui::Id::new("history_page_cache");
    let stamp = (app.stats_period, app.listening.plays.len());
    if let Some((s, p)) = ui.data(|d| d.get_temp::<((Period, usize), Arc<Page>)>(key)) {
        if s == stamp {
            return p;
        }
    }
    let now = crate::history::now_secs();
    let p = Arc::new(Page {
        stats: app.listening.stats(app.stats_period, now, TOP),
        recent: app
            .listening
            .newest_first()
            .take(RECENT)
            .map(|p| p.track.clone())
            .collect(),
    });
    ui.data_mut(|d| d.insert_temp(key, (stamp, p.clone())));
    p
}

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let mut out: Vec<UiAction> = Vec::new();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t("History"))
                .text_style(egui::TextStyle::Heading)
                .color(c_text()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !app.listening.is_empty()
                && widgets::pill(ui, crate::i18n::t("Clear history"), false).clicked()
            {
                let id = egui::Id::new("history_confirm_clear");
                ui.data_mut(|d| d.insert_temp(id, true));
            }
        });
    });
    clear_confirmation(ui, &mut out);

    if !app.config.record_history {
        common::message(
            ui,
            crate::i18n::t("History is turned off"),
            crate::i18n::t("Turn on “Keep listening history” in Settings to see your stats here."),
        );
    } else if app.listening.is_empty() {
        common::message(
            ui,
            crate::i18n::t("Nothing played yet"),
            crate::i18n::t("Songs you listen to show up here."),
        );
    } else {
        let page = page(ui, app);
        let ps = app.ps.clone();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (period, label) in [
                        (Period::Week, crate::i18n::t("7 days")),
                        (Period::Month, crate::i18n::t("30 days")),
                        (Period::All, crate::i18n::t("All time")),
                    ] {
                        if widgets::pill(ui, label, app.stats_period == period).clicked() {
                            app.stats_period = period;
                        }
                    }
                });
                ui.add_space(12.0);
                tiles(ui, &page.stats);
                ui.add_space(14.0);
                chart(ui, &page.stats.per_day);

                if !page.stats.top_artists.is_empty() {
                    widgets::section_title(ui, crate::i18n::t("Top artists"));
                    for (i, (artist, plays)) in page.stats.top_artists.iter().enumerate() {
                        if ranked_row(ui, i, &artist.name, *plays, artist.id.is_some()) {
                            if let Some(id) = &artist.id {
                                out.push(UiAction::Go(Route::Artist(id.clone())));
                            }
                        }
                    }
                }
                if !page.stats.top_tracks.is_empty() {
                    widgets::section_title(ui, crate::i18n::t("Top songs"));
                    let tracks: Vec<Track> = page
                        .stats
                        .top_tracks
                        .iter()
                        .map(|(t, _)| t.clone())
                        .collect();
                    common::track_rows(ui, &tracks, &ps, true, &mut out);
                }
                widgets::section_title(ui, crate::i18n::t("Recently played"));
                common::track_rows(ui, &page.recent, &ps, false, &mut out);
                ui.add_space(24.0);
            });
    }
    for a in out {
        app.run(a);
    }
}

/// "Clear all history?" with Clear / Cancel, shown after the Clear button was pressed.
fn clear_confirmation(ui: &mut Ui, out: &mut Vec<UiAction>) {
    let id = egui::Id::new("history_confirm_clear");
    if !ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    egui::Frame::new()
        .fill(theme::c_surface())
        .stroke(egui::Stroke::new(1.0, theme::c_border()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::t(
                        "Delete your whole listening history? This cannot be undone.",
                    ))
                    .color(c_text()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pill(ui, crate::i18n::t("Cancel"), false).clicked() {
                        ui.data_mut(|d| d.insert_temp(id, false));
                    }
                    if widgets::pill(ui, crate::i18n::t("Clear history"), true).clicked() {
                        out.push(UiAction::ClearHistory);
                        ui.data_mut(|d| d.insert_temp(id, false));
                    }
                });
            });
        });
    ui.add_space(6.0);
}

/// Four number cards: plays, listening time, songs, artists.
fn tiles(ui: &mut Ui, s: &Stats) {
    let gap = 12.0;
    let (cols, w) = widgets::grid_layout(ui.available_width() - 4.0, 170.0, gap);
    let cards = [
        (
            icon::PLAY_CIRCLE,
            s.plays.to_string(),
            crate::i18n::t("Plays"),
        ),
        (
            icon::CLOCK,
            duration_label(s.listened_secs),
            crate::i18n::t("Listening time"),
        ),
        (
            icon::MUSIC_NOTES,
            s.unique_tracks.to_string(),
            crate::i18n::t("Different songs"),
        ),
        (
            icon::MICROPHONE_STAGE,
            s.unique_artists.to_string(),
            crate::i18n::t("Different artists"),
        ),
    ];
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    for row in cards.chunks(cols) {
        ui.horizontal_top(|ui| {
            for (glyph, value, label) in row {
                tile(ui, glyph, value, label, w);
            }
        });
    }
}

fn tile(ui: &mut Ui, glyph: &str, value: &str, label: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, 84.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), theme::c_surface());
    p.text(
        pos2(rect.left() + 18.0, rect.top() + 24.0),
        Align2::LEFT_CENTER,
        glyph,
        FontId::new(18.0, theme::icons()),
        ACCENT,
    );
    p.text(
        pos2(rect.left() + 44.0, rect.top() + 24.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        c_text_dim(),
    );
    p.text(
        pos2(rect.left() + 18.0, rect.top() + 54.0),
        Align2::LEFT_CENTER,
        value,
        FontId::new(24.0, theme::bold_family()),
        c_text(),
    );
}

/// Bars for the plays of the last 14 days (today on the right).
fn chart(ui: &mut Ui, per_day: &[u32; 14]) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width() - 4.0, 96.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(14), theme::c_surface());
    let inner = rect.shrink2(vec2(18.0, 14.0));
    let plot = Rect::from_min_max(inner.min, pos2(inner.max.x, inner.max.y - 16.0));
    let max = per_day.iter().copied().max().unwrap_or(0).max(1) as f32;
    let slot = plot.width() / per_day.len() as f32;
    let bar_w = (slot * 0.62).clamp(3.0, 28.0);
    let hover = resp.hover_pos();
    let mut tip = None;
    for (i, &n) in per_day.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let h = if n == 0 {
            2.0
        } else {
            (plot.height() * n as f32 / max).max(4.0)
        };
        let bar = Rect::from_min_max(
            pos2(cx - bar_w / 2.0, plot.bottom() - h),
            pos2(cx + bar_w / 2.0, plot.bottom()),
        );
        let hot = hover.is_some_and(|p| (p.x - cx).abs() <= slot / 2.0 && rect.contains(p));
        let color = match (n, hot, i == per_day.len() - 1) {
            (0, _, _) => theme::c_border(),
            (_, true, _) | (_, _, true) => ACCENT,
            _ => ACCENT.gamma_multiply(0.55),
        };
        ui.painter().rect_filled(bar, CornerRadius::same(3), color);
        if hot {
            tip = Some((i, n));
        }
    }
    let label_y = rect.bottom() - 12.0;
    let faint = FontId::proportional(11.5);
    ui.painter().text(
        pos2(inner.left(), label_y),
        Align2::LEFT_CENTER,
        crate::i18n::t("14 days ago"),
        faint.clone(),
        theme::c_text_faint(),
    );
    ui.painter().text(
        pos2(inner.right(), label_y),
        Align2::RIGHT_CENTER,
        crate::i18n::t("Today"),
        faint,
        theme::c_text_faint(),
    );
    if let Some((i, n)) = tip {
        let ago = per_day.len() - 1 - i;
        let when = match ago {
            0 => crate::i18n::t("Today").to_owned(),
            1 => crate::i18n::t("Yesterday").to_owned(),
            n => crate::i18n::t("{} days ago").replace("{}", &n.to_string()),
        };
        resp.on_hover_text(format!("{when}: {n} {}", crate::i18n::t("plays")));
    }
}

/// One line of a ranking: position, name, play count. Returns whether it was clicked.
fn ranked_row(ui: &mut Ui, index: usize, name: &str, plays: u32, clickable: bool) -> bool {
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), sense);
    if ui.is_rect_visible(rect) {
        if resp.hovered() && clickable {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), theme::c_surface_hover());
        }
        let p = ui.painter();
        p.text(
            pos2(rect.left() + 20.0, rect.center().y),
            Align2::CENTER_CENTER,
            (index + 1).to_string(),
            FontId::proportional(13.0),
            theme::c_text_faint(),
        );
        let w = (rect.width() - 150.0).max(40.0);
        p.galley(
            pos2(rect.left() + 44.0, rect.center().y - 9.0),
            widgets::fit_text(
                p,
                name,
                FontId::new(14.0, theme::bold_family()),
                c_text(),
                w,
            ),
            c_text(),
        );
        p.text(
            pos2(rect.right() - 16.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{plays} {}", crate::i18n::t("plays")),
            FontId::proportional(13.0),
            c_text_dim(),
        );
    }
    clickable
        && resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        let _lang = crate::i18n::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        assert_eq!(duration_label(0), "< 1 min");
        assert_eq!(duration_label(59), "< 1 min");
        assert_eq!(duration_label(60 * 45), "45 min");
        assert_eq!(duration_label(3600 * 3 + 60 * 20), "3 h 20 min");
        assert_eq!(duration_label(3600 * 2), "2 h 0 min");
    }
}
