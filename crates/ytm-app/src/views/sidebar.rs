use eframe::egui::{self, pos2, vec2, Align2, CornerRadius, CursorIcon, FontId, Sense, Ui};
use egui_phosphor::regular as icon;

use crate::state::{AppState, Route};
use crate::theme::{self, c_surface_hover, c_text, c_text_dim, ACCENT};

fn nav_item(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    selected: bool,
    compact: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let hovered = resp.hovered();
    if selected || hovered {
        let fill = if selected {
            c_surface_hover()
        } else {
            theme::c_surface()
        };
        ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    }
    let fg = if selected || hovered {
        c_text()
    } else {
        c_text_dim()
    };
    let icon_x = if compact {
        rect.center().x
    } else {
        rect.left() + 28.0
    };
    ui.painter().text(
        pos2(icon_x, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(22.0, theme::icons()),
        if selected { ACCENT } else { fg },
    );
    if !compact {
        let font = FontId::new(15.0, theme::bold_family());
        ui.painter().text(
            pos2(rect.left() + 54.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font,
            fg,
        );
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    // The rail has no labels, so name the page on hover.
    if compact {
        resp.on_hover_text(label)
    } else {
        resp
    }
}

pub fn show(ui: &mut Ui, app: &mut AppState, compact: bool) {
    ui.add_space(14.0);
    // Logo
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    // The app icon itself (embedded PNG), so the sidebar matches the window/dock icon.
    let logo_x = if compact {
        rect.center().x
    } else {
        rect.left() + 28.0
    };
    let logo = egui::Rect::from_center_size(pos2(logo_x, rect.center().y), vec2(36.0, 36.0));
    ui.put(
        logo,
        egui::Image::new(egui::include_image!("../../assets/icons/128x128.png"))
            .fit_to_exact_size(logo.size())
            .show_loading_spinner(false),
    );
    if !compact {
        ui.painter().text(
            pos2(rect.left() + 54.0, rect.center().y),
            Align2::LEFT_CENTER,
            "Tunebox",
            FontId::new(21.0, theme::bold_family()),
            c_text(),
        );
    }
    ui.add_space(22.0);

    ui.spacing_mut().item_spacing.y = 4.0;
    let items = [
        (Route::Home, icon::HOUSE, crate::i18n::t("Home")),
        (Route::Explore, icon::COMPASS, crate::i18n::t("Explore")),
        (Route::Library, icon::BOOKS, crate::i18n::t("Library")),
        (
            Route::Playlists,
            icon::PLAYLIST,
            crate::i18n::t("Playlists"),
        ),
    ];
    for (route, glyph, label) in items {
        // Moods & genres are part of Explore.
        let selected = app.route == route
            || (route == Route::Explore
                && matches!(
                    app.route,
                    Route::Moods | Route::Mood(_) | Route::NewReleases | Route::Charts(_)
                ));
        if nav_item(ui, glyph, label, selected, compact).clicked() {
            app.navigate(route);
        }
    }
}
