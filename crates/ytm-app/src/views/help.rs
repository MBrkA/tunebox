//! Keyboard-shortcut cheat-sheet, opened with `?` and closed with `?` or Esc.

use eframe::egui::{self, vec2, Context, CornerRadius, RichText, Sense};

use crate::i18n::t;
use crate::state::AppState;
use crate::theme::{self, c_border, c_surface_hover, c_text, c_text_dim};

/// `Ctrl` is shown as `⌘` on macOS.
fn cmd() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}

/// `Alt` is called Option on macOS (spelled out: the bundled fonts have no `⌥` glyph).
fn alt() -> &'static str {
    if cfg!(target_os = "macos") {
        "Option"
    } else {
        "Alt"
    }
}

type Section = (&'static str, Vec<(String, &'static str)>);

/// Sections of (keys, what it does). Must match `TuneboxApp::shortcuts` in `app.rs`.
fn sections() -> Vec<Section> {
    let c = cmd();
    vec![
        (
            t("Playback"),
            vec![
                ("Space".into(), t("Play / pause")),
                (format!("{c} + →"), t("Next track")),
                (format!("{c} + ←"), t("Previous track")),
                ("→ / ←".into(), t("Seek 5 seconds")),
                (format!("{c} + ↑ / ↓"), t("Volume up / down")),
                ("M".into(), t("Mute")),
                ("S".into(), t("Shuffle")),
                ("R".into(), t("Repeat")),
                ("L".into(), t("Like the current song")),
            ],
        ),
        (
            t("Navigation"),
            vec![
                (format!("{c} + K  /  /"), t("Search")),
                (format!("{c} + 1…4"), t("Home, Explore, Library, Playlists")),
                (format!("{c} + ,"), t("Settings")),
                (format!("{} + ←", alt()), t("Back")),
                ("Q".into(), t("Queue")),
                ("N".into(), t("Open now playing")),
                ("Esc".into(), t("Close")),
                ("?".into(), t("Keyboard shortcuts")),
            ],
        ),
    ]
}

pub fn show(ctx: &Context, app: &mut AppState) {
    if !app.help_open {
        return;
    }
    let mut close = false;
    let height = ctx.content_rect().height();
    let width = crate::layout::dialog_width(ctx.content_rect().width()) + 40.0;
    super::dialogs::window("Keyboard shortcuts").show(ctx, |ui| {
        ui.set_width(width);
        ui.label(
            theme::bold(t("Keyboard shortcuts"))
                .size(20.0)
                .color(c_text()),
        );
        ui.add_space(6.0);
        // Scrolls when the window is too short to show every row.
        egui::ScrollArea::vertical()
            .max_height((height - 170.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // Compact rows: the default row height is made for buttons.
                ui.spacing_mut().interact_size.y = 22.0;
                for (title, rows) in sections() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(title).color(c_text_dim()).size(12.5));
                    ui.add_space(2.0);
                    for (keys, what) in rows {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(what).color(c_text()));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| key_cap(ui, &keys),
                            );
                        });
                    }
                }
            });
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
        let fill = if resp.hovered() {
            c_surface_hover()
        } else {
            theme::c_surface()
        };
        ui.painter().rect_filled(rect, CornerRadius::same(18), fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            t("Close"),
            egui::FontId::new(14.0, theme::bold_family()),
            c_text(),
        );
        close |= resp.clicked();
    });
    if close {
        app.help_open = false;
    }
}

fn key_cap(ui: &mut egui::Ui, keys: &str) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, c_border()))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(keys)
                    .color(c_text_dim())
                    .monospace()
                    .size(12.5),
            );
        });
}
