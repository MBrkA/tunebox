//! Modal dialogs: new / rename playlist.

use eframe::egui::{self, Align2, Context, Frame, Margin, RichText};

use crate::state::{AppState, UiAction};
use crate::theme::{self, c_text, c_text_dim};
use crate::widgets;

pub fn show(ctx: &Context, app: &mut AppState) {
    new_playlist(ctx, app);
    confirm_delete(ctx, app);
    confirm_restore(ctx, app);
}

/// Asks before a playlist is deleted; Esc or Cancel keeps it.
fn confirm_delete(ctx: &Context, app: &mut AppState) {
    let Some(id) = app.confirm_delete.clone() else {
        return;
    };
    let Some(playlist) = app.local.playlist(&id) else {
        app.confirm_delete = None;
        return;
    };
    let (title, n) = (playlist.title.clone(), playlist.song_count());
    let mut action = None;
    let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    window("Delete playlist").show(ctx, |ui| {
        ui.set_width(crate::layout::dialog_width(ctx.content_rect().width()));
        ui.label(
            theme::bold(crate::i18n::t("Delete this playlist?"))
                .size(20.0)
                .color(c_text()),
        );
        ui.label(
            RichText::new(format!("“{title}” — {n} {}", crate::i18n::t("songs")))
                .color(c_text_dim()),
        );
        ui.label(RichText::new(crate::i18n::t("This cannot be undone.")).color(c_text_dim()));
        ui.add_space(8.0);
        dialog_buttons(
            ui,
            egui_phosphor::regular::TRASH,
            crate::i18n::t("Delete"),
            || {
                action = Some(UiAction::ConfirmDeletePlaylist(id.clone()));
            },
            || cancel = true,
        );
    });
    if cancel {
        app.confirm_delete = None;
    }
    if let Some(a) = action {
        app.run(a);
    }
}

/// Asks before a restored backup replaces the library.
fn confirm_restore(ctx: &Context, app: &mut AppState) {
    let Some(lib) = &app.pending_restore else {
        return;
    };
    let summary = format!(
        "{} {}, {} {}, {} {}",
        lib.playlists.len(),
        crate::i18n::t("playlists"),
        lib.liked.len(),
        crate::i18n::t("liked songs"),
        lib.albums.len() + lib.artists.len() + lib.saved_playlists.len(),
        crate::i18n::t("saved items")
    );
    let mut apply = false;
    let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    window("Restore from backup").show(ctx, |ui| {
        ui.set_width(crate::layout::dialog_width(ctx.content_rect().width()));
        ui.label(
            theme::bold(crate::i18n::t("Replace your data?"))
                .size(20.0)
                .color(c_text()),
        );
        ui.label(
            RichText::new(crate::i18n::t(
                "Your current playlists, liked songs and saved items will be replaced by the backup. If it has settings, they are restored too.",
            ))
            .color(c_text_dim()),
        );
        ui.label(
            RichText::new(format!("{}: {summary}", crate::i18n::t("Backup contains")))
                .color(c_text_dim()),
        );
        ui.add_space(8.0);
        dialog_buttons(
            ui,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
            crate::i18n::t("Restore"),
            || apply = true,
            || cancel = true,
        );
    });
    if apply {
        app.run(UiAction::ApplyRestore);
        theme::apply_pref(ctx, theme::Pref::from_code(&app.config.theme));
    } else if cancel {
        app.pending_restore = None;
        app.pending_restore_settings = None;
    }
}

/// Primary action at the right edge, Cancel beside it; both the same height.
fn dialog_buttons(
    ui: &mut egui::Ui,
    glyph: &str,
    label: &str,
    mut on_confirm: impl FnMut(),
    mut on_cancel: impl FnMut(),
) {
    let row = egui::vec2(ui.available_width(), 44.0);
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if widgets::action_button(ui, glyph, label, true).clicked() {
                on_confirm();
            }
            if widgets::action_button(
                ui,
                egui_phosphor::regular::X,
                crate::i18n::t("Cancel"),
                false,
            )
            .clicked()
            {
                on_cancel();
            }
        },
    );
}

pub(super) fn window(title: &str) -> egui::Window<'static> {
    egui::Window::new(title.to_owned())
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .frame(
            Frame::new()
                .fill(theme::c_surface())
                .stroke(egui::Stroke::new(1.0, theme::c_border()))
                .corner_radius(egui::CornerRadius::same(18))
                .inner_margin(Margin::same(24))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 12],
                    blur: 40,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(180),
                }),
        )
        .title_bar(false)
}

fn new_playlist(ctx: &Context, app: &mut AppState) {
    let Some(np) = &mut app.new_playlist else {
        return;
    };
    let mut create = false;
    let mut cancel = false;
    let renaming = np.rename.is_some();
    window(crate::i18n::t("New playlist")).show(ctx, |ui| {
        ui.set_width(crate::layout::dialog_width(ctx.content_rect().width()));
        let heading = if renaming {
            crate::i18n::t("Rename playlist")
        } else {
            crate::i18n::t("New playlist")
        };
        ui.label(theme::bold(heading).size(20.0).color(c_text()));
        let n = np.tracks.len();
        if n > 0 {
            ui.label(
                RichText::new(format!(
                    "{n} song{} will be added.",
                    if n == 1 { "" } else { "s" }
                ))
                .color(c_text_dim()),
            );
        }
        ui.add_space(4.0);
        let r = widgets::text_field(ui, egui::Id::new("playlist_title"), &mut np.title, "Title");
        if !r.has_focus() && np.title.is_empty() {
            r.request_focus();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            create = true;
        }
        ui.add_space(8.0);
        // Same widget (and height) for both buttons; the primary action sits at the right edge.
        let row = egui::vec2(ui.available_width(), 44.0);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let (glyph, label) = if renaming {
                    (egui_phosphor::regular::CHECK, crate::i18n::t("Save"))
                } else {
                    (egui_phosphor::regular::PLUS, crate::i18n::t("Create"))
                };
                if widgets::action_button(ui, glyph, label, true).clicked() {
                    create = true;
                }
                if widgets::action_button(
                    ui,
                    egui_phosphor::regular::X,
                    crate::i18n::t("Cancel"),
                    false,
                )
                .clicked()
                {
                    cancel = true;
                }
            },
        );
    });
    if create {
        app.create_playlist();
    } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.new_playlist = None;
    }
}
