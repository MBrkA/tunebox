use eframe::egui::{self, Ui};
use egui_phosphor::regular as icon;

use crate::i18n::{self, t, Lang};
use crate::state::{AppState, UiAction};
use crate::theme::{self, c_text, c_text_dim, Mode};
use crate::widgets;

pub const DEVELOPER: &str = "MBrkA";

/// Refresh choices in minutes; 0 means "only when the app starts".
pub const REFRESH_CHOICES: [u32; 5] = [0, 15, 30, 60, 180];

pub fn refresh_label(minutes: u32) -> String {
    match minutes {
        0 => t("Never").to_owned(),
        m if m % 60 == 0 => format!("{} {}", m / 60, t("hours")),
        m => format!("{m} {}", t("minutes")),
    }
}

/// Widest the settings column grows; wider windows get side margins.
const MAX_WIDTH: f32 = 720.0;
/// Space kept for the control on the right of a row.
const CONTROL_WIDTH: f32 = 190.0;

/// A titled card holding settings rows.
fn card(ui: &mut Ui, glyph: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(14.0);
    egui::Frame::new()
        .fill(theme::c_surface())
        .stroke(egui::Stroke::new(1.0, theme::c_border()))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(glyph)
                        .font(egui::FontId::new(20.0, theme::icons()))
                        .color(c_text_dim()),
                );
                ui.label(theme::bold(title).size(16.0).color(c_text()));
            });
            ui.add_space(4.0);
            body(ui);
        });
}

/// Thin divider between rows.
fn divider(ui: &mut Ui) {
    ui.add_space(10.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::c_border());
    ui.add_space(10.0);
}

/// One setting: title and optional hint on the left, the control on the right.
fn row(ui: &mut Ui, title: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let left = (ui.available_width() - CONTROL_WIDTH).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(left, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(left);
                ui.add(
                    egui::Label::new(egui::RichText::new(title).size(14.5).color(c_text())).wrap(),
                );
                if let Some(hint) = hint {
                    ui.add_space(1.0);
                    caption(ui, hint);
                }
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// Pill-shaped switch; true when it was clicked this frame.
fn switch(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(44.0, 24.0), egui::Sense::click());
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off = if resp.hovered() {
        theme::c_strong()
    } else {
        theme::c_surface_active()
    };
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(12),
        off.lerp_to_gamma(theme::c_primary(), t),
    );
    let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), t);
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y),
        9.0,
        c_text_dim().lerp_to_gamma(theme::c_on_primary(), t),
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    let clicked = resp
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    if clicked {
        *on = !*on;
    }
    clicked
}

fn toggle_row(ui: &mut Ui, title: &str, hint: Option<&str>, value: &mut bool) -> bool {
    let mut changed = false;
    row(ui, title, hint, |ui| changed = switch(ui, title, value));
    changed
}

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let mut out = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // A bounded, centred column keeps rows readable on wide windows.
            let avail = ui.available_width();
            let width = avail.min(MAX_WIDTH);
            ui.horizontal_top(|ui| {
                ui.add_space(((avail - width) / 2.0).max(0.0));
                ui.vertical(|ui| {
                    ui.set_width(width);
                    page(ui, app, &mut out);
                });
            });
        });
    for a in out {
        app.run(a);
    }
}

fn page(ui: &mut Ui, app: &mut AppState, out: &mut Vec<UiAction>) {
    ui.add_space(8.0);
    ui.label(theme::bold(t("Settings")).size(28.0).color(c_text()));

    // ---- appearance & language ------------------------------------------
    card(ui, icon::PALETTE, t("Appearance"), |ui| {
        let mut mode = theme::mode();
        row(ui, t("Theme"), None, |ui| {
            // Right-to-left layout: added in reverse so Dark reads first.
            for (m, label) in [(Mode::Light, t("Light")), (Mode::Dark, t("Dark"))] {
                if widgets::pill(ui, label, mode == m).clicked() {
                    mode = m;
                }
            }
        });
        if mode != theme::mode() {
            theme::set_mode(ui.ctx(), mode);
            app.config.theme = mode.code().into();
            save_config(app);
        }

        divider(ui);
        let current = i18n::current();
        let mut chosen = current;
        row(
            ui,
            t("Interface language"),
            Some(t("Changes apply immediately.")),
            |ui| {
                egui::ComboBox::from_id_salt("ui_language")
                    .selected_text(current.native_name())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for l in Lang::ALL {
                            ui.selectable_value(&mut chosen, l, l.native_name());
                        }
                    });
            },
        );
        if chosen != current {
            i18n::set(chosen);
            app.config.ui_language = chosen.code().into();
            app.config.language = chosen.hl().into();
            save_config(app);
            app.language_changed(chosen.hl());
        }
    });

    // ---- content -----------------------------------------------------------
    card(ui, icon::ARROWS_CLOCKWISE, t("Content"), |ui| {
        let minutes = app.config.refresh_minutes;
        let mut chosen = minutes;
        row(
            ui,
            t("Refresh Home and Explore"),
            Some(t("A page you open again after this long is fetched again.")),
            |ui| {
                egui::ComboBox::from_id_salt("refresh_interval")
                    .selected_text(refresh_label(minutes))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for m in REFRESH_CHOICES {
                            ui.selectable_value(&mut chosen, m, refresh_label(m));
                        }
                    });
            },
        );
        if chosen != minutes {
            app.config.refresh_minutes = chosen;
            save_config(app);
        }
    });

    // ---- desktop -----------------------------------------------------------
    card(ui, icon::DESKTOP, t("Desktop"), |ui| {
        if toggle_row(ui, t("Show tray icon"), None, &mut app.config.tray_icon) {
            save_config(app);
            app.toast(t("Applies the next time Tunebox starts.").to_owned());
        }
        divider(ui);
        if toggle_row(
            ui,
            t("Show the song next to the tray icon"),
            Some(t(
                "Needs a tray host that shows labels, e.g. GNOME's AppIndicator extension.",
            )),
            &mut app.config.tray_label,
        ) {
            save_config(app);
        }
        divider(ui);
        if toggle_row(
            ui,
            t("Closing the window keeps Tunebox running in the tray"),
            Some(t("Only works while the tray icon is showing.")),
            &mut app.config.close_to_tray,
        ) {
            save_config(app);
            app.toast(t("Applies the next time Tunebox starts.").to_owned());
        }
        divider(ui);
        if toggle_row(
            ui,
            t("Notify when the song changes"),
            Some(t("Only shown while Tunebox is in the background.")),
            &mut app.config.notifications,
        ) {
            save_config(app);
        }
        divider(ui);
        if toggle_row(
            ui,
            t("Continue where you left off"),
            Some(t(
                "Restores the queue, song and position (paused) when Tunebox starts.",
            )),
            &mut app.config.restore_session,
        ) {
            save_config(app);
        }
    });

    // ---- application data ----------------------------------------------------
    card(ui, icon::DATABASE, t("Application data"), |ui| {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(t("Data folder"))
                .size(14.5)
                .color(c_text()),
        );
        ui.add_space(6.0);
        let dir = app.data_dir.clone();
        egui::Frame::new()
            .fill(theme::c_bg())
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(dir.display().to_string())
                            .color(c_text_dim())
                            .size(12.5)
                            .monospace(),
                    )
                    .wrap(),
                );
            });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::action_button(ui, icon::FOLDER_OPEN, t("Open folder"), false).clicked() {
                open_folder(&dir);
            }
            if widgets::action_button(ui, icon::FOLDER_SIMPLE, t("Change folder…"), false).clicked()
            {
                out.push(UiAction::MoveDataDir);
            }
        });
        ui.add_space(6.0);
        caption(
            ui,
            t("Your settings, library, playlists and covers are stored in this folder. Changing it copies everything to the new folder; the old folder is left in place."),
        );

        divider(ui);
        ui.horizontal_wrapped(|ui| {
            if widgets::action_button(ui, icon::EXPORT, t("Back up data"), false).clicked() {
                out.push(UiAction::BackupLibrary);
            }
            if widgets::action_button(
                ui,
                icon::ARROW_COUNTER_CLOCKWISE,
                t("Restore from backup"),
                false,
            )
            .clicked()
            {
                out.push(UiAction::RestoreLibrary);
            }
        });
        ui.add_space(6.0);
        caption(
            ui,
            t("A backup holds all playlists, liked songs, saved items and your settings (theme, language, refresh interval)."),
        );
    });

    // ---- about ---------------------------------------------------------------
    card(ui, icon::INFO, t("About"), |ui| {
        ui.add_space(4.0);
        about_row(ui, t("Version"), env!("CARGO_PKG_VERSION"));
        ui.add_space(4.0);
        about_row(ui, t("Developed by"), DEVELOPER);
    });
    ui.add_space(28.0);
}

fn caption(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(c_text_dim()).size(12.5)).wrap());
}

fn about_row(ui: &mut Ui, label: &str, value: &str) {
    row(ui, label, None, |ui| {
        ui.label(theme::bold(value).color(c_text()));
    });
}

/// Persist settings; a failure is shown as a toast but never blocks the change.
fn save_config(app: &mut AppState) {
    if let Err(e) = app.save_config() {
        tracing::warn!("could not save settings: {e}");
        app.toast(format!("Could not save settings: {e}"));
    }
}

/// Show a folder in the system file manager.
fn open_folder(dir: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        tracing::warn!("could not open {}: {e}", dir.display());
    }
}
