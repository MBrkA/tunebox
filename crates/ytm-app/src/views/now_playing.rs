//! Full-screen "now playing": dominant-colour backdrop, large artwork, lyrics.

use std::sync::Arc;

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, FontId, Rect, Sense, Ui, UiBuilder};
use egui_phosphor::regular as icon;

use crate::state::{AppState, Load, Route, UiAction};
use crate::theme::{self, c_bg, c_text, c_text_dim};
use crate::thumbs::ThumbLoader;
use crate::widgets;

pub fn show(ui: &mut Ui, app: &mut AppState, thumbs: &Arc<ThumbLoader>, open_t: f32) {
    let full = ui.max_rect();
    let ctx = ui.ctx().clone();
    app.ensure_lyrics();

    let Some(track) = app.ps.current_track().cloned() else {
        app.now_playing_open = false;
        return;
    };
    let art_url = widgets::best_thumbnail(&track.thumbnails).map(str::to_owned);

    // Backdrop: gradient from the artwork's dominant colour to the app background.
    let target = art_url
        .as_deref()
        .and_then(|u| thumbs.dominant_color(&ctx, &widgets::art_uri(&ctx, u, 420.0)))
        .unwrap_or(Color32::from_rgb(0x2A, 0x2A, 0x2A));
    let channel = |name: &str, v: u8| {
        ctx.animate_value_with_time(egui::Id::new(("np_color", name)), f32::from(v), 0.6)
    };
    let top = Color32::from_rgb(
        channel("r", target.r()) as u8,
        channel("g", target.g()) as u8,
        channel("b", target.b()) as u8,
    );
    widgets::vertical_gradient(ui.painter(), full, top, c_bg());

    // Content slides up into place while the page fades in.
    let inner = full
        .shrink2(vec2(40.0, 16.0))
        .translate(vec2(0.0, (1.0 - open_t) * 48.0));
    let mut out: Vec<UiAction> = Vec::new();
    let mut close = false;

    // Header
    let header = Rect::from_min_size(inner.min, vec2(inner.width(), 48.0));
    ui.scope_builder(UiBuilder::new().max_rect(header), |ui| {
        ui.horizontal_centered(|ui| {
            if widgets::icon_button(ui, icon::CARET_DOWN, 22.0, false, crate::i18n::t("Close"))
                .clicked()
            {
                close = true;
            }
            ui.label(
                theme::bold(crate::i18n::t("Now playing"))
                    .size(15.0)
                    .color(c_text()),
            );
        });
    });

    let body = Rect::from_min_max(pos2(inner.left(), header.bottom() + 8.0), inner.max);
    let two_col = body.width() >= 820.0;
    let art_size = if two_col {
        (body.height() - 120.0)
            .clamp(220.0, 460.0)
            .min(body.width() * 0.45)
    } else {
        (body.height() - 200.0)
            .clamp(160.0, 320.0)
            .min(body.width())
    };

    let left = Rect::from_min_size(
        pos2(
            if two_col {
                body.left() + 24.0
            } else {
                body.center().x - art_size / 2.0
            },
            body.top() + 8.0,
        ),
        vec2(art_size, body.height() - 8.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(art_size, art_size), Sense::hover());
        ui.painter().rect_filled(
            rect.translate(vec2(0.0, 10.0)).expand(2.0),
            CornerRadius::same(18),
            Color32::from_black_alpha(90),
        );
        widgets::paint_art(
            ui,
            rect,
            art_url.as_deref(),
            CornerRadius::same(theme::CARD_RADIUS + 2),
        );
        ui.add_space(18.0);
        ui.add(egui::Label::new(theme::bold(&track.title).size(26.0).color(c_text())).truncate());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, a) in track.artists.iter().enumerate() {
                if i > 0 {
                    ui.label(egui::RichText::new(", ").color(c_text_dim()).size(16.0));
                }
                let font = FontId::proportional(16.0);
                match &a.id {
                    Some(id) => {
                        if widgets::link_text(ui, &a.name, font, c_text_dim()).clicked() {
                            out.push(UiAction::Go(Route::Artist(id.clone())));
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new(&a.name).font(font).color(c_text_dim()));
                    }
                }
            }
        });
        ui.add_space(6.0);
        let liked = app.is_liked(&track.video_id);
        let tip = if liked {
            crate::i18n::t("Remove from liked songs")
        } else {
            crate::i18n::t("Add to liked songs")
        };
        let clicked = if liked {
            widgets::icon_button_filled(ui, icon::HEART, 24.0, true, tip).clicked()
        } else {
            widgets::icon_button(ui, icon::HEART, 24.0, false, tip).clicked()
        };
        if clicked {
            out.push(UiAction::ToggleLike(track.clone()));
        }
        if let Some(album) = &track.album {
            ui.horizontal(|ui| match &album.id {
                Some(id) => {
                    if widgets::link_text(
                        ui,
                        &album.name,
                        FontId::proportional(14.0),
                        theme::c_text_faint(),
                    )
                    .clicked()
                    {
                        out.push(UiAction::Go(Route::Album(id.clone())));
                    }
                }
                None => {
                    ui.label(egui::RichText::new(&album.name).color(theme::c_text_faint()));
                }
            });
        }
    });

    if two_col {
        let right = Rect::from_min_max(pos2(left.right() + 56.0, body.top() + 8.0), body.max);
        ui.scope_builder(UiBuilder::new().max_rect(right), |ui| lyrics(ui, app));
    } else {
        // Narrow windows: lyrics below the artwork would not fit; they are a wide-layout feature.
    }

    if close {
        app.now_playing_open = false;
    }
    for a in out {
        app.run(a);
    }
}

fn lyrics(ui: &mut Ui, app: &mut AppState) {
    ui.label(
        theme::bold(crate::i18n::t("Lyrics"))
            .size(18.0)
            .color(c_text()),
    );
    ui.add_space(10.0);
    match app.lyrics.load.clone() {
        Load::Idle | Load::Loading => {
            ui.add(egui::Spinner::new().color(c_text_dim()));
        }
        Load::Failed(_) => {
            ui.label(
                egui::RichText::new(crate::i18n::t("Could not load lyrics.")).color(c_text_dim()),
            );
        }
        Load::Ready(l) => match l.as_ref() {
            None => {
                ui.label(
                    egui::RichText::new(crate::i18n::t("No lyrics available for this track."))
                        .color(c_text_dim()),
                );
            }
            Some(l) => {
                egui::ScrollArea::vertical()
                    .id_salt(("lyrics", app.lyrics.video_id.clone()))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for line in l.text.lines() {
                            if line.trim().is_empty() {
                                ui.add_space(14.0);
                            } else {
                                ui.add(
                                    egui::Label::new(
                                        theme::bold(line)
                                            .size(22.0)
                                            .color(Color32::from_white_alpha(235)),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                        if let Some(src) = &l.source {
                            ui.add_space(18.0);
                            ui.label(
                                egui::RichText::new(src)
                                    .color(theme::c_text_faint())
                                    .size(12.0),
                            );
                        }
                        ui.add_space(40.0);
                    });
            }
        },
    }
}
