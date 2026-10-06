use eframe::egui::{
    self, pos2, vec2, Align, Align2, Area, CornerRadius, FontId, Frame, Id, Key, Margin, Modifiers,
    Order, Rect, Sense, Stroke, TextEdit, Ui,
};
use egui_phosphor::regular as icon;

use crate::state::{AppState, Route};
use crate::theme::{self, c_surface, c_surface_hover, c_text, c_text_dim};
use crate::widgets;

const BOX_HEIGHT: f32 = 46.0;

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let search_id = Id::new("search_box");
    ui.horizontal_centered(|ui| {
        ui.add_space(4.0);
        let can_back = app.can_go_back();
        ui.add_enabled_ui(can_back, |ui| {
            if widgets::icon_button(ui, icon::CARET_LEFT, 20.0, false, crate::i18n::t("Back"))
                .clicked()
            {
                app.back();
            }
        });
        ui.add_space(8.0);

        let width = ui.available_width().min(620.0);
        let (box_rect, _) = ui.allocate_exact_size(vec2(width, BOX_HEIGHT), Sense::hover());
        search_box(ui, app, box_rect, search_id);

        // Settings sits at the far right of the search row.
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            let on = app.route == Route::Settings;
            if widgets::icon_button(ui, icon::GEAR, 22.0, on, crate::i18n::t("Settings")).clicked()
                && !on
            {
                app.navigate(Route::Settings);
            }
            // Right-to-left: this lands to the left of the gear.
            if app.restart_required() {
                let tip = crate::i18n::t("Restart required to apply your changes.");
                let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon::WARNING,
                    egui::FontId::new(22.0, theme::icons()),
                    egui::Color32::from_rgb(0xF2, 0xB1, 0x34),
                );
                if resp
                    .on_hover_text(tip)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                    && app.route != Route::Settings
                {
                    app.navigate(Route::Settings);
                }
            }
        });
    });
}

fn search_box(ui: &mut Ui, app: &mut AppState, rect: Rect, id: Id) {
    // Ctrl/Cmd+K focuses the box from anywhere.
    if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K))
        || std::mem::take(&mut app.search_focus_requested)
    {
        ui.memory_mut(|m| m.request_focus(id));
    }

    let popup_open = app.search.show_suggestions && !app.search.suggestions.is_empty();
    if popup_open && ui.memory(|m| m.has_focus(id)) {
        let n = app.search.suggestions.len();
        let (down, up, esc) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let sel = &mut app.search.suggest_sel;
        if down {
            *sel = Some(sel.map_or(0, |s| (s + 1) % n));
        }
        if up {
            *sel = Some(sel.map_or(n - 1, |s| (s + n - 1) % n));
        }
        if esc {
            app.search.show_suggestions = false;
        }
    }

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let focused = child.memory(|m| m.has_focus(id));
    let frame = Frame::new()
        .fill(c_surface())
        .corner_radius(CornerRadius::same((BOX_HEIGHT / 2.0) as u8))
        .stroke(if focused {
            Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.8))
        } else {
            Stroke::new(1.0, theme::c_border())
        })
        .inner_margin(Margin::symmetric(16, 0));
    let mut submit: Option<String> = None;
    frame.show(&mut child, |ui| {
        ui.set_height(BOX_HEIGHT - 2.0);
        ui.set_width(rect.width() - 34.0);
        ui.horizontal_centered(|ui| {
            ui.label(
                egui::RichText::new(icon::MAGNIFYING_GLASS)
                    .family(theme::icons())
                    .size(18.0)
                    .color(c_text_dim()),
            );
            let clear_w = 28.0;
            let edit = TextEdit::singleline(&mut app.search.input)
                .id(id)
                .hint_text(crate::i18n::t("Search songs, albums, artists"))
                .font(FontId::proportional(15.0))
                .frame(egui::Frame::NONE)
                .vertical_align(Align::Center)
                .desired_width(ui.available_width() - clear_w);
            let resp = ui.add(edit);
            if resp.changed() {
                app.search_text_changed();
            }
            if resp.gained_focus() && !app.search.input.is_empty() {
                app.search.show_suggestions = true;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let picked = app
                    .search
                    .suggest_sel
                    .and_then(|i| app.search.suggestions.get(i).cloned());
                submit = Some(picked.unwrap_or_else(|| app.search.input.clone()));
            }
            if !app.search.input.is_empty()
                && widgets::icon_button(ui, icon::X, 14.0, false, crate::i18n::t("Clear")).clicked()
            {
                app.search.input.clear();
                app.search_text_changed();
                ui.memory_mut(|m| m.request_focus(id));
            }
        });
    });
    if let Some(q) = submit {
        app.submit_search(&q);
        child.memory_mut(|m| m.surrender_focus(id));
    }
    suggestions_popup(ui, app, rect, id);
}

fn suggestions_popup(ui: &mut Ui, app: &mut AppState, anchor: Rect, id: Id) {
    let focused = ui.memory(|m| m.has_focus(id));
    let pointer_in_popup = app
        .search
        .popup_rect
        .zip(ui.input(|i| i.pointer.hover_pos()))
        .is_some_and(|(r, p)| r.contains(p));
    let open = app.search.show_suggestions
        && !app.search.suggestions.is_empty()
        && (focused || pointer_in_popup);
    if !open {
        app.search.popup_rect = None;
        return;
    }
    let mut picked: Option<String> = None;
    let suggestions = app.search.suggestions.clone();
    let sel = app.search.suggest_sel;
    let resp = Area::new(Id::new("suggestions"))
        .order(Order::Foreground)
        .fixed_pos(pos2(anchor.left(), anchor.bottom() + 6.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(c_surface())
                .stroke(Stroke::new(1.0, theme::c_border()))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::same(6))
                .shadow(ui.visuals().popup_shadow)
                .show(ui, |ui| {
                    ui.set_width(anchor.width() - 12.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (i, s) in suggestions.iter().enumerate() {
                        let (rect, r) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                        if r.hovered() || sel == Some(i) {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(10),
                                c_surface_hover(),
                            );
                        }
                        ui.painter().text(
                            pos2(rect.left() + 18.0, rect.center().y),
                            Align2::CENTER_CENTER,
                            icon::MAGNIFYING_GLASS,
                            FontId::new(16.0, theme::icons()),
                            c_text_dim(),
                        );
                        let p = ui.painter();
                        p.galley(
                            pos2(rect.left() + 42.0, rect.center().y - 9.0),
                            widgets::fit_text(
                                p,
                                s,
                                FontId::proportional(14.5),
                                c_text(),
                                rect.width() - 56.0,
                            ),
                            c_text(),
                        );
                        if r.clicked() {
                            picked = Some(s.clone());
                        }
                    }
                });
        });
    app.search.popup_rect = Some(resp.response.rect);
    if let Some(q) = picked {
        app.submit_search(&q);
    }
}
