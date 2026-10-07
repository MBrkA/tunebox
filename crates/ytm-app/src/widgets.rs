//! Custom widgets drawn with `Painter` where egui's stock widgets cannot
//! produce the design: icon buttons, pills, seek bar, art, cards, track rows.

use std::sync::Arc;

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Align2, Color32, CornerRadius, CursorIcon, FontId, Galley,
    Painter, Rect, Response, Sense, Ui, Vec2,
};
use egui_phosphor::regular as icon;
use ytm_api::{pick_thumbnail, Thumbnail, Track};

use crate::theme::{self, c_bg, c_border, c_surface_hover, c_text, c_text_dim, ACCENT};
use crate::thumbs::sized_url;

pub const ROW_HEIGHT: f32 = 56.0;

/// Lays out `text` on one line, ellipsised to `max_width`.
pub fn fit_text(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

pub fn format_time(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Round, borderless icon button. `active` tints it with the accent colour.
pub fn icon_button(ui: &mut Ui, glyph: &str, size: f32, active: bool, tooltip: &str) -> Response {
    icon_button_in(ui, glyph, theme::icons(), size, active, tooltip)
}

/// An icon button that can be switched off: when `enabled` is false it is drawn dim, gives no hover
/// feedback, shows no pointer cursor and never reports a click.
pub fn icon_button_enabled(
    ui: &mut Ui,
    glyph: &str,
    size: f32,
    enabled: bool,
    tooltip: &str,
) -> Response {
    if enabled {
        return icon_button(ui, glyph, size, false, tooltip);
    }
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 16.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(size, theme::icons()),
            theme::c_text_faint().gamma_multiply(0.45),
        );
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, false, tooltip));
    resp
}

/// Like [`icon_button`] but from the filled icon set (e.g. a liked heart).
pub fn icon_button_filled(
    ui: &mut Ui,
    glyph: &str,
    size: f32,
    active: bool,
    tooltip: &str,
) -> Response {
    icon_button_in(ui, glyph, theme::fill_icons(), size, active, tooltip)
}

fn icon_button_in(
    ui: &mut Ui,
    glyph: &str,
    family: egui::FontFamily,
    size: f32,
    active: bool,
    tooltip: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 16.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        if hovered {
            ui.painter()
                .circle_filled(rect.center(), rect.width() / 2.0, c_surface_hover());
        }
        let color = if active {
            ACCENT
        } else if hovered {
            c_text()
        } else {
            c_text_dim()
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(size, family),
            color,
        );
    }
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    resp.on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// The big white play/pause disc.
pub fn play_button(ui: &mut Ui, playing: bool, loading: bool, diameter: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let hovered = resp.hovered();
    let fill = if hovered {
        theme::c_primary_hover()
    } else {
        theme::c_primary()
    };
    let scale = if hovered { 1.04 } else { 1.0 };
    let r = diameter / 2.0 * scale;
    ui.painter().circle_filled(rect.center(), r, fill);
    if loading {
        let inner = Rect::from_center_size(rect.center(), Vec2::splat(diameter * 0.5));
        egui::Spinner::new().color(c_bg()).paint_at(ui, inner);
    } else {
        let glyph = if playing {
            egui_phosphor::fill::PAUSE
        } else {
            egui_phosphor::fill::PLAY
        };
        // Optical centring: the play triangle sits slightly right of centre.
        let nudge = if playing { 0.0 } else { diameter * 0.03 };
        ui.painter().text(
            rect.center() + vec2(nudge, 0.0),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(diameter * 0.5, theme::fill_icons()),
            c_bg(),
        );
    }
    let label = if playing {
        crate::i18n::t("Pause")
    } else {
        crate::i18n::t("Play")
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Rounded chip used for filters.
pub fn pill(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let font = FontId::new(13.5, theme::bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 32.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (bg, fg) = match (selected, resp.hovered()) {
        (true, _) => (theme::c_primary(), theme::c_on_primary()),
        (false, true) => (theme::c_surface_active(), c_text()),
        (false, false) => (c_surface_hover(), c_text()),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(17), bg);
    ui.painter()
        .galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, fg);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, text));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Horizontal progress/seek/volume bar. Returns the fraction to seek to when
/// the user clicks or releases a drag.
pub fn seek_bar(ui: &mut Ui, fraction: f32, width: f32, enabled: bool) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(width, 18.0),
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    let active = enabled && (resp.hovered() || resp.dragged());
    let pointer_frac = resp
        .interact_pointer_pos()
        .or_else(|| resp.hover_pos())
        .map(|p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let shown = if resp.dragged() {
        pointer_frac.unwrap_or(fraction)
    } else {
        fraction
    }
    .clamp(0.0, 1.0);

    if ui.is_rect_visible(rect) {
        let h = if active { 6.0 } else { 4.0 };
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), h));
        let radius = CornerRadius::same((h / 2.0) as u8);
        ui.painter().rect_filled(track, radius, theme::c_strong());
        let mut filled = track;
        filled.set_right(track.left() + track.width() * shown);
        let color = if active {
            theme::ACCENT_HOVER
        } else {
            c_text()
        };
        ui.painter()
            .rect_filled(filled, radius, if enabled { color } else { c_border() });
        if active {
            ui.painter()
                .circle_filled(pos2(filled.right(), rect.center().y), 7.0, c_text());
        }
    }
    if resp.hovered() && enabled {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if enabled && (resp.drag_stopped() || resp.clicked()) {
        return pointer_frac;
    }
    None
}

/// The exact URI used to display `url` at `logical` points (shared with colour extraction).
pub fn art_uri(ctx: &egui::Context, url: &str, logical: f32) -> String {
    sized_url(url, pixel_bucket(ctx, logical))
}

/// Smallest source size (px) that is crisp at `logical` points on this display.
fn pixel_bucket(ctx: &egui::Context, logical: f32) -> u32 {
    let needed = logical * ctx.pixels_per_point();
    [60u32, 120, 226, 302, 544, 800]
        .into_iter()
        .find(|b| *b as f32 >= needed)
        .unwrap_or(800)
}

pub fn best_thumbnail(thumbs: &[Thumbnail]) -> Option<&str> {
    pick_thumbnail(thumbs, 0).map(|t| t.url.as_str())
}

pub fn paint_art(ui: &mut Ui, rect: Rect, url: Option<&str>, radius: impl Into<CornerRadius>) {
    paint_art_impl(ui, rect, url, radius.into(), true);
}

/// `place`: lay the image out with `ui.put` (moves the layout cursor) instead of only painting it.
/// Collage cells must not move the cursor, or they push the neighbouring widgets around.
fn paint_art_impl(ui: &mut Ui, rect: Rect, url: Option<&str>, radius: CornerRadius, place: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, radius, c_surface_hover());
    let Some(url) = url else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon::MUSIC_NOTES,
            FontId::new(rect.width() * 0.35, theme::icons()),
            theme::c_text_faint(),
        );
        return;
    };
    let mut image = egui::Image::new(art_uri(ui.ctx(), url, rect.width()))
        .corner_radius(radius)
        .show_loading_spinner(false)
        .fit_to_exact_size(rect.size())
        .maintain_aspect_ratio(false);
    if url.contains("ytimg.com") {
        // 16:9 / 4:3 video frames: crop to the centre square of the real picture, which needs the
        // texture size (known once it has loaded; until then nothing is painted anyway).
        if let Ok(egui::load::TexturePoll::Ready { texture }) =
            image.load_for_size(ui.ctx(), rect.size())
        {
            image = image.uv(video_square_uv(texture.size));
        }
    }
    if place {
        ui.put(rect, image);
    } else {
        image.paint_at(ui, rect);
    }
}

/// The part of a YouTube video thumbnail (texture `size` in pixels) that shows as a square, as uv
/// coordinates. 16:9 frames (maxres/mq) fill the height and are cropped left and right. 4:3 frames
/// (sddefault/hqdefault) carry black bars above and below a 16:9 picture, so the bars are cut
/// first. Cutting by the real size keeps the crop square, so the image is not stretched.
fn video_square_uv(size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let aspect = size.x / size.y;
    let (y0, y1) = if aspect < 1.5 {
        (0.125, 0.875)
    } else {
        (0.0, 1.0)
    };
    let side_px = size.y * (y1 - y0);
    let half_w = (side_px / size.x / 2.0).min(0.5);
    Rect::from_min_max(pos2(0.5 - half_w, y0), pos2(0.5 + half_w, y1))
}

/// 2×2 collage of song covers. With fewer than four covers the remaining cells stay empty.
pub fn paint_mosaic(ui: &mut Ui, rect: Rect, covers: &[String], radius: CornerRadius) {
    if covers.is_empty() {
        return paint_art(ui, rect, None, radius);
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, radius, c_surface_hover());
    let half = rect.width() / 2.0;
    for i in 0..4usize {
        let min = rect.min + vec2((i % 2) as f32 * half, (i / 2) as f32 * half);
        let cell = Rect::from_min_size(min, Vec2::splat(half));
        // Only the outer corner of each cell is rounded.
        let (a, b) = (radius.nw, 0u8);
        let cr = match i {
            0 => CornerRadius {
                nw: a,
                ne: b,
                sw: b,
                se: b,
            },
            1 => CornerRadius {
                nw: b,
                ne: radius.ne,
                sw: b,
                se: b,
            },
            2 => CornerRadius {
                nw: b,
                ne: b,
                sw: radius.sw,
                se: b,
            },
            _ => CornerRadius {
                nw: b,
                ne: b,
                sw: b,
                se: radius.se,
            },
        };
        match covers.get(i) {
            Some(url) => paint_art_impl(ui, cell, Some(url), cr, false),
            None => {
                ui.painter().rect_filled(cell, cr, c_surface_hover());
            }
        }
    }
}

/// Grid card for albums, playlists and artists.
pub fn card(
    ui: &mut Ui,
    art_url: Option<&str>,
    title: &str,
    subtitle: &str,
    width: f32,
    circle: bool,
) -> Response {
    card_with_covers(ui, art_url, &[], title, subtitle, width, circle)
}

/// [`card`] that shows a [`paint_mosaic`] instead of the single image when `covers` is not empty.
pub fn card_with_covers(
    ui: &mut Ui,
    art_url: Option<&str>,
    covers: &[String],
    title: &str,
    subtitle: &str,
    width: f32,
    circle: bool,
) -> Response {
    let pad = 10.0;
    let art_size = width - pad * 2.0;
    let height = pad + art_size + 12.0 + 20.0 + 18.0 + pad;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::CARD_RADIUS),
            theme::c_surface(),
        );
    }
    let art_rect = Rect::from_min_size(rect.min + vec2(pad, pad), Vec2::splat(art_size));
    let radius = if circle {
        CornerRadius::same((art_size / 2.0) as u8)
    } else {
        CornerRadius::same(theme::ART_RADIUS)
    };
    if covers.is_empty() {
        paint_art(ui, art_rect, art_url, radius);
    } else {
        paint_mosaic(ui, art_rect, covers, radius);
    }

    let text_w = art_size;
    let y = art_rect.bottom() + 12.0;
    let align_center = circle;
    let place = |painter: &Painter, galley: Arc<Galley>, y: f32| {
        let x = if align_center {
            art_rect.center().x - galley.size().x / 2.0
        } else {
            art_rect.left()
        };
        painter.galley(pos2(x, y), galley, c_text());
    };
    let p = ui.painter();
    place(
        p,
        fit_text(
            p,
            title,
            FontId::new(14.0, theme::bold_family()),
            c_text(),
            text_w,
        ),
        y,
    );
    place(
        p,
        fit_text(
            p,
            subtitle,
            FontId::proportional(12.5),
            c_text_dim(),
            text_w,
        ),
        y + 20.0,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// One song in a list. Returns the row response (click = play).
pub fn track_row(
    ui: &mut Ui,
    track: &Track,
    number: Option<usize>,
    current: bool,
    playing: bool,
) -> Response {
    track_row_sense(ui, track, number, current, playing, Sense::click())
}

/// `track_row` with a custom `Sense` (the queue rows add dragging).
pub fn track_row_sense(
    ui: &mut Ui,
    track: &Track,
    number: Option<usize>,
    current: bool,
    playing: bool,
    sense: Sense,
) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), sense);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if resp.hovered() || current {
        let fill = if current {
            theme::c_surface()
        } else {
            c_surface_hover()
        };
        ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    }
    let x0 = rect.left() + 8.0;
    let mut x = x0;
    if let Some(n) = number {
        ui.painter().text(
            pos2(x + 12.0, rect.center().y),
            Align2::CENTER_CENTER,
            (n + 1).to_string(),
            FontId::proportional(13.0),
            theme::c_text_faint(),
        );
        x += 34.0;
    }
    let art_rect = Rect::from_center_size(pos2(x + 20.0, rect.center().y), Vec2::splat(40.0));
    paint_art(
        ui,
        art_rect,
        best_thumbnail(&track.thumbnails),
        CornerRadius::same(8),
    );
    if current {
        ui.painter().rect_filled(
            art_rect,
            CornerRadius::same(8),
            Color32::from_black_alpha(140),
        );
        let glyph = if playing {
            icon::SPEAKER_HIGH
        } else {
            icon::PAUSE
        };
        ui.painter().text(
            art_rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(18.0, theme::icons()),
            ACCENT,
        );
    }
    x = art_rect.right() + 14.0;

    let duration = track.duration_secs.map(|d| format_time(u64::from(d)));
    let right_w = 64.0;
    let text_w = (rect.right() - x - right_w - 12.0).max(40.0);
    let p = ui.painter();
    let title_color = if current { ACCENT } else { c_text() };
    p.galley(
        pos2(x, rect.center().y - 19.0),
        fit_text(
            p,
            &track.title,
            FontId::new(14.0, theme::bold_family()),
            title_color,
            text_w,
        ),
        title_color,
    );
    let sub = match &track.album {
        Some(a) if !a.name.is_empty() => format!("{} • {}", track.artist_line(), a.name),
        _ => track.artist_line(),
    };
    p.galley(
        pos2(x, rect.center().y + 2.0),
        fit_text(p, &sub, FontId::proportional(12.5), c_text_dim(), text_w),
        c_text_dim(),
    );
    if let Some(d) = duration {
        p.text(
            pos2(rect.right() - 16.0, rect.center().y),
            Align2::RIGHT_CENTER,
            d,
            FontId::proportional(13.0),
            c_text_dim(),
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Clickable text that underlines on hover (artist / album links).
pub fn link_text(ui: &mut Ui, text: &str, font: FontId, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    let color = if resp.hovered() { c_text() } else { color };
    ui.painter()
        .galley_with_override_text_color(rect.min, galley, color);
    if resp.hovered() {
        let y = rect.bottom() - 1.0;
        ui.painter().line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            egui::Stroke::new(1.0, color),
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Pill button with a leading icon. `primary` = filled white, otherwise subtle.
pub fn action_button(ui: &mut Ui, glyph: &str, text: &str, primary: bool) -> Response {
    let font = FontId::new(14.0, theme::bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 64.0, 44.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (bg, fg) = match (primary, resp.hovered()) {
        (true, false) => (theme::c_primary(), theme::c_on_primary()),
        (true, true) => (theme::c_primary_hover(), theme::c_on_primary()),
        (false, false) => (theme::c_surface_active(), c_text()),
        (false, true) => (theme::c_strong(), c_text()),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(22), bg);
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(18.0, theme::icons()),
        fg,
    );
    ui.painter().galley_with_override_text_color(
        pos2(rect.left() + 44.0, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A single-line text input with the app's look: padded, rounded, accent border while focused.
pub fn text_field(ui: &mut Ui, id: egui::Id, text: &mut String, hint: &str) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let stroke = if focused {
        egui::Stroke::new(1.5, ACCENT.gamma_multiply(0.85))
    } else {
        egui::Stroke::new(1.0, c_border())
    };
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .hint_text(hint)
                    .font(FontId::proportional(15.0))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            )
        })
        .inner
}

/// A plain navigation button: icon on the left, label next to it, neutral colours (no tint).
/// Used for the "New releases / Charts / Moods & genres" shortcuts on Explore.
pub fn icon_tile(ui: &mut Ui, glyph: &str, label: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if resp.hovered() {
            theme::c_surface_active()
        } else {
            c_surface_hover()
        };
        ui.painter().rect_filled(rect, CornerRadius::same(14), bg);
        ui.painter().text(
            pos2(rect.left() + 30.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(24.0, theme::icons()),
            if resp.hovered() {
                c_text()
            } else {
                theme::c_text_dim()
            },
        );
        let p = ui.painter();
        p.galley(
            pos2(rect.left() + 58.0, rect.center().y - 10.0),
            fit_text(
                p,
                label,
                FontId::new(15.0, theme::bold_family()),
                c_text(),
                width - 76.0,
            ),
            c_text(),
        );
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Height of a mood / genre tile.
pub const MOOD_TILE_HEIGHT: f32 = 64.0;

/// A mood / genre tile of the given `width`: rounded, tinted with its own colour, with a stripe on the
/// left (`color` is YouTube's 0xAARRGGBB stripe colour). Long names are ellipsised.
pub fn mood_chip(ui: &mut Ui, title: &str, color: Option<u32>, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, MOOD_TILE_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let [r, g, b] = color.map_or([0x70, 0x70, 0x70], |c| {
            [(c >> 16) as u8, (c >> 8) as u8, c as u8]
        });
        let accent = Color32::from_rgb(r, g, b);
        // base surface with a faint wash of the tile's own colour
        let base = if resp.hovered() {
            theme::c_surface_active()
        } else {
            c_surface_hover()
        };
        let wash = if resp.hovered() { 0.20 } else { 0.11 };
        let bg = Color32::from_rgb(
            lerp_u8(base.r(), r, wash),
            lerp_u8(base.g(), g, wash),
            lerp_u8(base.b(), b, wash),
        );
        ui.painter().rect_filled(rect, CornerRadius::same(14), bg);
        let stripe =
            Rect::from_min_size(rect.min + vec2(0.0, 14.0), vec2(6.0, rect.height() - 28.0));
        ui.painter()
            .rect_filled(stripe, CornerRadius::same(3), accent);
        let p = ui.painter();
        p.galley(
            pos2(rect.left() + 26.0, rect.center().y - 10.0),
            fit_text(
                p,
                title,
                FontId::new(15.0, theme::bold_family()),
                c_text(),
                width - 44.0,
            ),
            c_text(),
        );
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8
}

pub struct SplitButton {
    /// The labelled left part (the primary action).
    pub main: Response,
    /// The ▾ part on the right; attach a dropdown to it.
    pub arrow: Response,
}

/// A pill made of a primary action and a ▾ part for a dropdown, side by side.
/// `active` draws it highlighted (e.g. while a mode is on).
pub fn split_button(ui: &mut Ui, glyph: &str, label: &str, active: bool) -> SplitButton {
    let font = FontId::new(14.0, theme::bold_family());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let (main_w, arrow_w, h) = (galley.size().x + 64.0, 40.0, 44.0);
    let (rect, _) = ui.allocate_exact_size(vec2(main_w + arrow_w, h), Sense::hover());
    let main_rect = Rect::from_min_size(rect.min, vec2(main_w, h));
    let arrow_rect = Rect::from_min_size(pos2(rect.left() + main_w, rect.top()), vec2(arrow_w, h));
    let id = ui.id().with(("split", label));
    let main = ui.interact(main_rect, id.with("main"), Sense::click());
    let arrow = ui.interact(arrow_rect, id.with("arrow"), Sense::click());

    if ui.is_rect_visible(rect) {
        let (base, hover, fg) = if active {
            (
                theme::c_primary(),
                theme::c_primary_hover(),
                theme::c_on_primary(),
            )
        } else {
            (theme::c_surface_active(), theme::c_strong(), c_text())
        };
        let r = 22u8;
        ui.painter().rect_filled(rect, CornerRadius::same(r), base);
        let hovered_part = |part: &Response, corners: CornerRadius, area: Rect| {
            if part.hovered() || part.is_pointer_button_down_on() {
                ui.painter().rect_filled(area, corners, hover);
            }
        };
        hovered_part(
            &main,
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            main_rect,
        );
        hovered_part(
            &arrow,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            },
            arrow_rect,
        );
        let divider = if active {
            theme::c_on_primary().gamma_multiply(0.16)
        } else {
            c_text().gamma_multiply(0.11)
        };
        ui.painter().line_segment(
            [
                pos2(arrow_rect.left(), rect.top() + 11.0),
                pos2(arrow_rect.left(), rect.bottom() - 11.0),
            ],
            egui::Stroke::new(1.0, divider),
        );
        ui.painter().text(
            pos2(main_rect.left() + 26.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(18.0, theme::icons()),
            fg,
        );
        ui.painter().galley_with_override_text_color(
            pos2(
                main_rect.left() + 44.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            fg,
        );
        ui.painter().text(
            arrow_rect.center(),
            Align2::CENTER_CENTER,
            icon::CARET_DOWN,
            FontId::new(16.0, theme::icons()),
            fg,
        );
    }
    main.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    arrow.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            crate::i18n::t("More actions"),
        )
    });
    SplitButton {
        main: main.on_hover_cursor(CursorIcon::PointingHand),
        arrow: arrow
            .on_hover_text("More actions")
            .on_hover_cursor(CursorIcon::PointingHand),
    }
}

/// The gap nearest to `pointer_y`, given the y of every gap line as laid out this frame
/// (`gaps[g]` is the line above row `g`; the last entry is below the last row).
pub fn drop_gap(pointer_y: f32, gaps: &[f32]) -> usize {
    gaps.iter()
        .enumerate()
        .min_by(|a, b| (a.1 - pointer_y).abs().total_cmp(&(b.1 - pointer_y).abs()))
        .map_or(0, |(g, _)| g)
}

/// Where row `from` ends up when dropped in gap `gap`, or `None` if that leaves it in place.
pub fn drop_target(from: usize, gap: usize) -> Option<usize> {
    let to = if gap > from { gap - 1 } else { gap };
    (to != from).then_some(to)
}

pub struct EditRow {
    /// The row's rectangle as laid out this frame.
    pub rect: Rect,
    /// Drag this to reorder.
    pub handle: Response,
    pub remove: Response,
}

/// A playlist row in edit mode: grip handle on the left, remove button on the right.
/// `dimmed` is used for the row currently being dragged.
pub fn edit_row(ui: &mut Ui, track: &Track, index: usize, dimmed: bool) -> EditRow {
    let width = ui.available_width();
    let (rect, row) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::hover());
    let id = ui.id().with(("edit_row", index));
    let handle_rect = Rect::from_min_size(rect.min + vec2(2.0, 0.0), vec2(40.0, ROW_HEIGHT));
    let remove_rect = Rect::from_center_size(
        pos2(rect.right() - 28.0, rect.center().y),
        Vec2::splat(40.0),
    );
    let handle = ui.interact(handle_rect, id.with("grip"), Sense::drag());
    let remove = ui.interact(remove_rect, id.with("remove"), Sense::click());

    if ui.is_rect_visible(rect) {
        if row.hovered() || handle.dragged() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), theme::c_surface());
        }
        let grip = if handle.hovered() || handle.dragged() {
            c_text()
        } else {
            theme::c_text_faint()
        };
        ui.painter().text(
            handle_rect.center(),
            Align2::CENTER_CENTER,
            icon::DOTS_SIX_VERTICAL,
            FontId::new(22.0, theme::icons()),
            grip,
        );
        let art_rect =
            Rect::from_center_size(pos2(rect.left() + 66.0, rect.center().y), Vec2::splat(40.0));
        paint_art(
            ui,
            art_rect,
            best_thumbnail(&track.thumbnails),
            CornerRadius::same(8),
        );
        let x = art_rect.right() + 14.0;
        let text_w = (remove_rect.left() - x - 8.0).max(40.0);
        let p = ui.painter();
        p.galley(
            pos2(x, rect.center().y - 19.0),
            fit_text(
                p,
                &track.title,
                FontId::new(14.0, theme::bold_family()),
                c_text(),
                text_w,
            ),
            c_text(),
        );
        p.galley(
            pos2(x, rect.center().y + 2.0),
            fit_text(
                p,
                &track.artist_line(),
                FontId::proportional(12.5),
                c_text_dim(),
                text_w,
            ),
            c_text_dim(),
        );
        if remove.hovered() {
            ui.painter().circle_filled(
                remove_rect.center(),
                18.0,
                Color32::from_rgba_unmultiplied(0xFF, 0x00, 0x33, 40),
            );
        }
        let rm = if remove.hovered() {
            ACCENT
        } else {
            c_text_dim()
        };
        ui.painter().text(
            remove_rect.center(),
            Align2::CENTER_CENTER,
            icon::X,
            FontId::new(18.0, theme::icons()),
            rm,
        );
        if dimmed {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(10), c_bg().gamma_multiply(0.67));
        }
    }
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(if handle.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    let remove = remove
        .on_hover_text("Remove from playlist")
        .on_hover_cursor(CursorIcon::PointingHand);
    EditRow {
        rect,
        handle,
        remove,
    }
}

/// A menu entry with a leading icon. The glyph is drawn from the dedicated icon family so the
/// text font can never shadow it; `color` tints both parts (default: the menu's text colour).
pub fn icon_menu_item(ui: &mut Ui, glyph: &str, label: &str, color: Option<Color32>) -> Response {
    let color = color.unwrap_or_else(|| ui.visuals().widgets.inactive.fg_stroke.color);
    let mut job = LayoutJob::default();
    job.append(
        glyph,
        0.0,
        egui::TextFormat::simple(FontId::new(15.0, theme::icons()), color),
    );
    job.append(
        label,
        10.0,
        egui::TextFormat::simple(FontId::proportional(14.0), color),
    );
    ui.button(job)
}

pub fn section_title(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text)
            .text_style(theme::title_style())
            .color(c_text()),
    );
}

/// Columns and card width for a responsive grid of cards.
pub fn grid_layout(avail: f32, target: f32, gap: f32) -> (usize, f32) {
    let cols = (((avail + gap) / (target + gap)).floor() as usize).max(1);
    let w = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
    (cols, w.floor())
}

/// Vertical gradient from `top` to `bottom` filling `rect`.
pub fn vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    use egui::epaint::{Mesh, Shape};
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_covers_are_cropped_to_a_true_square() {
        for (size, y0, y1) in [
            (vec2(1280.0, 720.0), 0.0, 1.0),    // 16:9 (maxresdefault)
            (vec2(640.0, 480.0), 0.125, 0.875), // 4:3 with bars (sddefault)
            (vec2(480.0, 360.0), 0.125, 0.875), // 4:3 with bars (hqdefault)
            (vec2(320.0, 180.0), 0.0, 1.0),     // 16:9 (mqdefault)
        ] {
            let uv = video_square_uv(size);
            assert_eq!((uv.min.y, uv.max.y), (y0, y1), "{size:?}");
            let (w_px, h_px) = (uv.width() * size.x, uv.height() * size.y);
            assert!(
                (w_px - h_px).abs() < 0.5,
                "{size:?}: {w_px} x {h_px} is not square"
            );
            assert!(
                (uv.center().x - 0.5).abs() < 1e-6,
                "{size:?}: centred horizontally"
            );
        }
        // an unknown size shows the whole picture instead of dividing by zero
        let all = video_square_uv(vec2(0.0, 0.0));
        assert_eq!((all.width(), all.height()), (1.0, 1.0));
        assert_eq!(video_square_uv(vec2(100.0, 0.0)).width(), 1.0);
        // a (nearly) square texture never asks for more than its width
        let sq = video_square_uv(vec2(100.0, 100.0));
        assert!(sq.min.x >= 0.0 && sq.max.x <= 1.0);
    }

    #[test]
    fn drop_gap_snaps_to_the_nearest_line_between_rows() {
        let gaps = [98.0, 158.0, 218.0, 278.0]; // three rows
        assert_eq!(drop_gap(98.0, &gaps), 0);
        assert_eq!(drop_gap(126.0, &gaps), 0);
        assert_eq!(drop_gap(129.0, &gaps), 1);
        assert_eq!(drop_gap(50.0, &gaps), 0, "above the list");
        assert_eq!(drop_gap(9999.0, &gaps), 3, "below the list");
        assert_eq!(drop_gap(120.0, &[]), 0, "empty list is safe");
    }

    #[test]
    fn drop_target_accounts_for_the_removed_row() {
        // moving row 2: dropping in the gap above or below it is a no-op
        assert_eq!(drop_target(2, 2), None);
        assert_eq!(drop_target(2, 3), None);
        assert_eq!(drop_target(2, 0), Some(0));
        assert_eq!(drop_target(2, 1), Some(1));
        assert_eq!(drop_target(2, 4), Some(3));
        assert_eq!(drop_target(2, 5), Some(4), "the very end");
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(65), "1:05");
        assert_eq!(format_time(3725), "1:02:05");
    }

    #[test]
    fn grid_fills_width_without_overflow() {
        for avail in [320.0f32, 700.0, 1000.0, 1500.0] {
            let (cols, w) = grid_layout(avail, 176.0, 16.0);
            assert!(cols >= 1);
            let used = cols as f32 * w + (cols as f32 - 1.0) * 16.0;
            assert!(used <= avail + 0.5, "{avail}: used {used}");
            assert!(w >= 120.0 || cols == 1);
        }
        assert_eq!(grid_layout(100.0, 176.0, 16.0).0, 1);
    }
}
