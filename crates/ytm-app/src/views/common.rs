//! Pieces shared by several pages.

use std::hash::Hash;

use eframe::egui::{self, vec2, Id, Rect, Response, ScrollArea, Ui};
use egui_phosphor::regular as icon;
use ytm_api::{SearchItem, Section, Track};
use ytm_player::PlayerState;

use crate::state::{Route, UiAction};
use crate::theme::{self, c_text_dim};
use crate::widgets;

pub fn centered_spinner(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height().min(600.0) * 0.3);
        ui.add(egui::Spinner::new().size(28.0).color(theme::ACCENT));
    });
}

/// Error panel with a retry button; returns true when crate::i18n::t("Try again") was clicked.
pub fn error_panel(ui: &mut Ui, title: &str, detail: &str) -> bool {
    let mut retry = false;
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height().min(600.0) * 0.25);
        ui.label(egui::RichText::new(title).text_style(egui::TextStyle::Heading));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(detail).color(c_text_dim()));
        ui.add_space(14.0);
        retry = widgets::pill(ui, crate::i18n::t("Try again"), true).clicked();
    });
    retry
}

pub fn message(ui: &mut Ui, title: &str, body: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height().min(600.0) * 0.28);
        ui.label(egui::RichText::new(title).text_style(egui::TextStyle::Heading));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(body).color(c_text_dim()).size(15.0));
    });
}

/// egui's menu style squeezes items to 2 px of horizontal and 0 px of vertical padding, so the hover
/// highlight hugs the text. Call first thing inside every menu / popup closure.
pub fn style_menu(ui: &mut Ui) {
    ui.spacing_mut().button_padding = vec2(14.0, 8.0);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.set_min_width(190.0);
    let w = &mut ui.style_mut().visuals.widgets;
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = egui::CornerRadius::same(8);
    }
}

/// What the song context menu needs to know; published once per frame by the
/// app (`publish_menu_data`) so any list can show the menu.
#[derive(Debug, Clone, Default)]
pub struct MenuData {
    /// `(id, title)` of the playlists on this device.
    pub local_playlists: Vec<(String, String)>,
    /// Ids of liked songs.
    pub liked: std::collections::HashSet<String>,
}

pub fn publish_menu_data(ctx: &egui::Context, data: MenuData) {
    ctx.data_mut(|d| d.insert_temp(Id::new("menu_data"), std::sync::Arc::new(data)));
}

fn menu_data(ctx: &egui::Context) -> std::sync::Arc<MenuData> {
    ctx.data(|d| d.get_temp::<std::sync::Arc<MenuData>>(Id::new("menu_data")))
        .unwrap_or_default()
}

/// Context menu for a song (right click).
pub fn track_menu(resp: &Response, track: &Track, out: &mut Vec<UiAction>) {
    track_menu_with(resp, track, out, |_, _| {});
}

/// Like [`track_menu`], with extra entries (e.g. crate::i18n::t("Remove from playlist")) at the top.
pub fn track_menu_with(
    resp: &Response,
    track: &Track,
    out: &mut Vec<UiAction>,
    extra: impl FnOnce(&mut Ui, &mut Vec<UiAction>),
) {
    let md = menu_data(&resp.ctx);
    let mut popup = egui::Popup::context_menu(resp);
    // Dev only (`--open-menu`): keep the first song's menu open so it can be screenshotted.
    if crate::DEV_OPEN_MENU.load(std::sync::atomic::Ordering::Relaxed) {
        static FIRST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        if *FIRST.get_or_init(|| track.video_id.clone()) == track.video_id {
            popup = popup
                .open(true)
                .at_position(resp.rect.center() + vec2(40.0, 10.0));
        }
    }
    popup.show(|ui| {
        style_menu(ui);
        let before = out.len();
        extra(ui, out);
        if out.len() != before {
            ui.close();
            return;
        }
        if ui.button(crate::i18n::t("Play next")).clicked() {
            out.push(UiAction::PlayNext(track.clone()));
            ui.close();
        }
        if ui.button(crate::i18n::t("Add to queue")).clicked() {
            out.push(UiAction::Enqueue(vec![track.clone()]));
            ui.close();
        }
        if ui.button(crate::i18n::t("Start radio")).clicked() {
            out.push(UiAction::Radio(track.video_id.clone()));
            ui.close();
        }
        ui.separator();
        let liked = md.liked.contains(&track.video_id);
        let label = if liked {
            crate::i18n::t("Remove from liked songs")
        } else {
            crate::i18n::t("Add to liked songs")
        };
        if ui.button(label).clicked() {
            out.push(UiAction::ToggleLike(track.clone()));
            ui.close();
        }
        ui.menu_button(crate::i18n::t("Add to playlist"), |ui| {
            style_menu(ui);
            for (id, title) in &md.local_playlists {
                if ui.button(title).clicked() {
                    out.push(UiAction::AddToLocalPlaylist {
                        playlist_id: id.clone(),
                        title: title.clone(),
                        tracks: vec![track.clone()],
                    });
                    ui.close();
                }
            }
            if !md.local_playlists.is_empty() {
                ui.separator();
            }
            if ui.button(crate::i18n::t("New playlist…")).clicked() {
                out.push(UiAction::NewPlaylistFor(vec![track.clone()]));
                ui.close();
            }
        });
        ui.separator();
        if let Some(id) = track.artists.iter().find_map(|a| a.id.clone()) {
            if ui.button(crate::i18n::t("Go to artist")).clicked() {
                out.push(UiAction::Go(Route::Artist(id)));
                ui.close();
            }
        }
        if let Some(id) = track.album.as_ref().and_then(|a| a.id.clone()) {
            if ui.button(crate::i18n::t("Go to album")).clicked() {
                out.push(UiAction::Go(Route::Album(id)));
                ui.close();
            }
        }
    });
}

/// crate::i18n::t("Save") / crate::i18n::t("Saved") toggle for albums, artists and playlists (kept on this device).
pub fn save_button(ui: &mut Ui, saved: bool, item: SearchItem, out: &mut Vec<UiAction>) {
    let (glyph, label) = if saved {
        (icon::CHECK, crate::i18n::t("Saved"))
    } else {
        (icon::BOOKMARK_SIMPLE, crate::i18n::t("Save"))
    };
    if widgets::action_button(ui, glyph, label, false).clicked() {
        out.push(UiAction::ToggleSaved(item));
    }
}

/// A plain list of song rows (not virtualised; for album/playlist/top-songs sized lists).
pub fn track_rows(
    ui: &mut Ui,
    tracks: &[Track],
    ps: &PlayerState,
    numbered: bool,
    out: &mut Vec<UiAction>,
) {
    let current = ps.current_track().map(|t| t.video_id.as_str());
    let playing = ps.status == ytm_player::Status::Playing;
    for (i, t) in tracks.iter().enumerate() {
        let is_current = current == Some(t.video_id.as_str());
        let r = widgets::track_row(ui, t, numbered.then_some(i), is_current, playing);
        if r.clicked() {
            out.push(UiAction::PlayTracks(tracks.to_vec(), i));
        }
        track_menu(&r, t, out);
    }
}

pub fn item_card(
    ui: &mut Ui,
    item: &SearchItem,
    w: f32,
    siblings: &[Track],
    out: &mut Vec<UiAction>,
) -> Response {
    match item {
        SearchItem::Track(t) => {
            let r = widgets::card(
                ui,
                widgets::best_thumbnail(&t.thumbnails),
                &t.title,
                &t.artist_line(),
                w,
                false,
            );
            if r.clicked() {
                let start = siblings
                    .iter()
                    .position(|s| s.video_id == t.video_id)
                    .unwrap_or(0);
                out.push(UiAction::PlayTracks(siblings.to_vec(), start));
            }
            track_menu(&r, t, out);
            r
        }
        SearchItem::Album(a) => {
            let sub = std::iter::once(a.kind.clone())
                .chain(a.artists.first().map(|x| x.name.clone()))
                .chain(a.year.clone())
                .collect::<Vec<_>>()
                .join(" • ");
            let r = widgets::card(
                ui,
                widgets::best_thumbnail(&a.thumbnails),
                &a.title,
                &sub,
                w,
                false,
            );
            if r.clicked() {
                out.push(UiAction::Go(Route::Album(a.browse_id.clone())));
            }
            r
        }
        SearchItem::Artist(a) => {
            let sub = if a.subtitle.is_empty() {
                crate::i18n::t("Artist")
            } else {
                &a.subtitle
            };
            let r = widgets::card(
                ui,
                widgets::best_thumbnail(&a.thumbnails),
                &a.name,
                sub,
                w,
                true,
            );
            if r.clicked() {
                out.push(UiAction::Go(Route::Artist(a.browse_id.clone())));
            }
            r
        }
        SearchItem::Playlist(p) => {
            let r = widgets::card_with_covers(
                ui,
                widgets::best_thumbnail(&p.thumbnails),
                &p.covers,
                &p.title,
                &p.subtitle,
                w,
                false,
            );
            if r.clicked() {
                out.push(UiAction::Go(Route::Playlist(p.playlist_id.clone())));
            }
            r
        }
    }
}

/// Mood / genre tiles in an aligned grid: equal-sized, columns line up, the column count follows the
/// window width. `max_rows` limits how many rows are shown (e.g. a teaser on Explore).
/// Returns the tile rectangles (used by tests).
pub fn mood_chips(
    ui: &mut Ui,
    moods: &[ytm_api::MoodCategory],
    max_rows: Option<usize>,
    out: &mut Vec<UiAction>,
) -> Vec<Rect> {
    const GAP: f32 = 12.0;
    // Leave room for the scrollbar gutter that is part of the available width.
    let (cols, w) = widgets::grid_layout((ui.available_width() - 16.0).max(200.0), 210.0, GAP);
    let shown = max_rows.map_or(moods.len(), |r| moods.len().min(r * cols));
    let mut rects = Vec::new();
    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
    for row in moods[..shown].chunks(cols) {
        ui.horizontal(|ui| {
            for m in row {
                let r = widgets::mood_chip(ui, &m.title, m.color, w);
                rects.push(r.rect);
                if r.clicked() {
                    out.push(UiAction::Go(Route::Mood(m.params.clone())));
                }
            }
        });
    }
    rects
}

const CARD_W: f32 = 176.0;
const CARD_GAP: f32 = 12.0;
/// How long a page-to-page slide takes.
const SLIDE_SECONDS: f64 = 0.32;

/// An in-flight eased slide of a carousel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub from: f32,
    pub to: f32,
    pub start: f64,
}

impl Slide {
    /// Offset at time `now` and whether the slide has finished.
    pub fn at(&self, now: f64) -> (f32, bool) {
        let p = ((now - self.start) / SLIDE_SECONDS).clamp(0.0, 1.0) as f32;
        let eased = egui::emath::easing::cubic_out(p);
        (self.from + (self.to - self.from) * eased, p >= 1.0)
    }
}

/// Where a click on an arrow should take the carousel: about one visible page further (keeping a card
/// of context), aligned to whole cards, never past either end. `dir` is +1 (right) or -1 (left).
pub fn page_target(from: f32, viewport: f32, max: f32, dir: f32) -> f32 {
    let step = CARD_W + CARD_GAP;
    let per_page = ((viewport / step).floor() - 1.0).max(1.0);
    let raw = from + dir * per_page * step;
    ((raw / step).round() * step).clamp(0.0, max.max(0.0))
}

/// Per-carousel state kept between frames.
#[derive(Debug, Clone, Copy)]
struct CarouselState {
    offset: f32,
    max: f32,
    viewport: f32,
    slide: Option<Slide>,
    measured: bool,
}

impl Default for CarouselState {
    fn default() -> Self {
        Self {
            offset: 0.0,
            max: 0.0,
            viewport: 800.0,
            slide: None,
            measured: false,
        }
    }
}

/// What a carousel did this frame (used by tests).
#[derive(Debug, Clone, Copy, Default)]
pub struct CarouselInfo {
    pub prev: Option<Rect>,
    pub next: Option<Rect>,
    pub can_prev: bool,
    pub can_next: bool,
    pub offset: f32,
    pub max: f32,
}

/// A titled horizontal carousel. The arrows slide it by a page with an eased animation and switch off
/// at either end (and disappear when everything fits).
pub fn carousel(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    section: &Section,
    out: &mut Vec<UiAction>,
) -> CarouselInfo {
    let id = Id::new(id);
    let state_key = id.with("carousel_state");
    let mut st: CarouselState = ui.data(|d| d.get_temp(state_key)).unwrap_or_default();
    let now = ui.input(|i| i.time);

    // Advance a running slide; this frame's forced offset comes from it.
    let mut forced = None;
    if let Some(slide) = st.slide {
        let (value, done) = slide.at(now);
        forced = Some(value);
        if done {
            st.slide = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    // Where the carousel is heading (clicks stack: pressing twice quickly goes two pages).
    let base = st.slide.map_or(st.offset, |s| s.to);
    let mut info = CarouselInfo {
        can_prev: base > 0.5,
        can_next: base < st.max - 0.5,
        offset: st.offset,
        max: st.max,
        ..CarouselInfo::default()
    };
    let overflow = !st.measured || st.max > 0.5;

    // Rows without a title (e.g. a featured card) get no header and no arrows.
    if section.title.is_empty() {
        ui.add_space(8.0);
    } else {
        let mut dir = 0.0;
        ui.horizontal(|ui| {
            widgets::section_title(ui, &section.title);
            if !overflow {
                return; // everything fits: nothing to scroll
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let next = widgets::icon_button_enabled(
                    ui,
                    icon::CARET_RIGHT,
                    16.0,
                    info.can_next,
                    crate::i18n::t("Next"),
                );
                if next.clicked() {
                    dir = 1.0;
                }
                info.next = Some(next.rect);
                let prev = widgets::icon_button_enabled(
                    ui,
                    icon::CARET_LEFT,
                    16.0,
                    info.can_prev,
                    crate::i18n::t("Previous"),
                );
                if prev.clicked() {
                    dir = -1.0;
                }
                info.prev = Some(prev.rect);
            });
        });
        if dir != 0.0 {
            let to = page_target(base, st.viewport, st.max, dir);
            if (to - base).abs() > 0.5 {
                st.slide = Some(Slide {
                    from: st.offset,
                    to,
                    start: now,
                });
                ui.ctx().request_repaint();
            }
        }
    }

    let siblings: Vec<Track> = section
        .items
        .iter()
        .filter_map(|i| match i {
            SearchItem::Track(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    let mut area = ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = forced {
        area = area.horizontal_scroll_offset(offset);
    }
    let output = area.show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
        ui.horizontal_top(|ui| {
            for item in &section.items {
                let _ = item_card(ui, item, CARD_W, &siblings, out);
            }
        });
    });
    st.offset = output.state.offset.x;
    st.viewport = output.inner_rect.width();
    st.max = (output.content_size.x - output.inner_rect.width()).max(0.0);
    if !st.measured {
        st.measured = true;
        ui.ctx().request_repaint(); // arrows were drawn from guesses on the very first frame
    }
    ui.data_mut(|d| d.insert_temp(state_key, st));
    info.offset = st.offset;
    info.max = st.max;
    info
}

/// All sections of a page as carousels.
pub fn sections(ui: &mut Ui, salt: &str, sections: &[Section], out: &mut Vec<UiAction>) {
    for (i, s) in sections.iter().enumerate() {
        let _ = carousel(ui, (salt, i), s, out);
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::RawInput;

    #[test]
    fn menus_get_comfortable_padding_instead_of_egui_menu_defaults() {
        let ctx = egui::Context::default();
        let mut seen = None;
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            // what egui's own `menu_style` does to every menu
            ui.spacing_mut().button_padding = vec2(2.0, 0.0);
            style_menu(ui);
            seen = Some((ui.spacing().button_padding, ui.min_rect().width()));
        });
        out.textures_delta.clear();
        let (padding, min_width) = seen.unwrap();
        assert_eq!(
            padding,
            vec2(14.0, 8.0),
            "hover highlight must have room around the text"
        );
        assert!(min_width >= 190.0, "menus are not cramped");
    }

    fn mood(i: usize, title: &str) -> ytm_api::MoodCategory {
        ytm_api::MoodCategory {
            title: title.into(),
            params: format!("p{i}"),
            color: Some(0xFF33_66CC),
        }
    }

    fn layout(width: f32, moods: &[ytm_api::MoodCategory], max_rows: Option<usize>) -> Vec<Rect> {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut rects = Vec::new();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(width, 800.0),
            )),
            ..RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut actions = Vec::new();
            rects = mood_chips(ui, moods, max_rows, &mut actions);
        });
        out.textures_delta.clear();
        rects
    }

    #[test]
    fn mood_tiles_are_uniform_and_aligned_at_every_window_width() {
        let names = [
            "Chill",
            "Country & Americana",
            "Dance & electronic",
            "Turkish folk & traditional",
            "Jazz",
            "Pop",
            "R&B & soul",
        ];
        let moods: Vec<_> = (0..23).map(|i| mood(i, names[i % names.len()])).collect();
        for width in [520.0, 700.0, 1000.0, 1400.0] {
            let rects = layout(width, &moods, None);
            assert_eq!(rects.len(), moods.len());
            let (w, h) = (rects[0].width(), rects[0].height());
            assert!(
                rects
                    .iter()
                    .all(|r| (r.width() - w).abs() < 0.01 && (r.height() - h).abs() < 0.01),
                "every tile has the same size at width {width}"
            );
            assert!(w >= 150.0, "tiles are not narrow: {w} at width {width}");
            // columns line up: only a handful of distinct x positions, each reused by every row
            let mut xs: Vec<i32> = rects.iter().map(|r| r.left().round() as i32).collect();
            xs.sort_unstable();
            xs.dedup();
            let cols = xs.len();
            let mut ys: Vec<i32> = rects.iter().map(|r| r.top().round() as i32).collect();
            ys.dedup();
            assert_eq!(
                rects.len().div_ceil(cols),
                ys.len(),
                "rows are complete grid rows at width {width}"
            );
            assert!(
                rects.iter().all(|r| r.right() <= width - 8.0),
                "nothing overflows at width {width}"
            );
        }
    }

    #[test]
    fn mood_teaser_shows_only_the_requested_rows() {
        let moods: Vec<_> = (0..40).map(|i| mood(i, "Mood")).collect();
        let rects = layout(1000.0, &moods, Some(2));
        let mut ys: Vec<i32> = rects.iter().map(|r| r.top().round() as i32).collect();
        ys.dedup();
        assert_eq!(ys.len(), 2, "two rows");
        assert!(rects.len() < 40);
        assert!(
            layout(1000.0, &moods[..3], Some(2)).len() == 3,
            "fewer moods than the cap"
        );
    }

    // ---- carousel scrolling ----

    #[test]
    fn page_target_moves_a_page_snaps_to_cards_and_never_overshoots() {
        let step = CARD_W + CARD_GAP;
        // viewport shows 5 cards: a page is 4 cards (one card of context stays visible)
        let viewport = 5.0 * step + 4.0;
        let max = 20.0 * step - viewport;
        let t = page_target(0.0, viewport, max, 1.0);
        assert_eq!(t, 4.0 * step);
        assert_eq!(
            page_target(t, viewport, max, -1.0),
            0.0,
            "left undoes right"
        );
        assert_eq!(
            page_target(0.0, viewport, max, -1.0),
            0.0,
            "cannot go before the start"
        );
        assert_eq!(
            page_target(max - 10.0, viewport, max, 1.0),
            max,
            "the last page lands exactly on the end"
        );
        assert_eq!(
            page_target(3.0, viewport, 0.0, 1.0),
            0.0,
            "nothing to scroll when everything fits"
        );
        assert!(
            page_target(0.0, 100.0, 500.0, 1.0) >= step,
            "even a tiny viewport moves a whole card"
        );
    }

    #[test]
    fn slide_eases_out_and_finishes_exactly_on_target() {
        let s = Slide {
            from: 100.0,
            to: 700.0,
            start: 10.0,
        };
        assert_eq!(s.at(10.0), (100.0, false));
        let (quarter, done) = s.at(10.0 + SLIDE_SECONDS / 4.0);
        assert!(
            !done && quarter > 100.0 + 0.25 * 600.0,
            "ease-out is ahead of linear early on: {quarter}"
        );
        assert!(quarter < 700.0);
        assert_eq!(s.at(10.0 + SLIDE_SECONDS), (700.0, true));
        assert_eq!(s.at(99.0), (700.0, true), "stays finished");
    }

    /// Runs a carousel of `n` albums at `width` through real egui frames with a controllable clock.
    struct Carousel {
        ctx: egui::Context,
        section: Section,
        width: f32,
        time: f64,
        info: CarouselInfo,
    }

    impl Carousel {
        fn new(n: usize, width: f32) -> Self {
            let items = (0..n)
                .map(|i| {
                    SearchItem::Album(ytm_api::AlbumSummary {
                        browse_id: format!("MPRE{i}"),
                        title: format!("Album {i}"),
                        ..Default::default()
                    })
                })
                .collect();
            let mut c = Self {
                ctx: egui::Context::default(),
                section: Section {
                    title: "Row".into(),
                    items,
                },
                width,
                time: 1.0,
                info: CarouselInfo::default(),
            };
            crate::theme::install(&c.ctx);
            for _ in 0..4 {
                c.frame(vec![]);
            }
            c
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(self.width, 600.0),
                )),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let (section, info) = (&self.section, &mut self.info);
            let mut out = self.ctx.run_ui(input, |ui| {
                let mut actions = Vec::new();
                *info = carousel(ui, "row", section, &mut actions);
            });
            out.textures_delta.clear();
        }

        fn click(&mut self, at: egui::Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at)]);
            for pressed in [true, false] {
                self.frame(vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }]);
            }
        }

        fn settle(&mut self) {
            for _ in 0..40 {
                self.frame(vec![]); // 40 frames ≈ 0.67 s, longer than a slide
            }
        }
    }

    #[test]
    fn arrows_slide_gradually_not_instantly() {
        let mut c = Carousel::new(24, 900.0);
        assert!(c.info.max > 500.0, "content overflows: {:?}", c.info);
        let at = c.info.next.expect("right arrow").center();
        c.click(at);
        let mut samples = Vec::new();
        for _ in 0..30 {
            c.frame(vec![]);
            samples.push(c.info.offset);
        }
        let end = *samples.last().unwrap();
        assert!(end > 300.0, "it moved a page: {end}");
        assert!(
            samples[0] < end * 0.6,
            "the first frame is not the final position: {samples:?}"
        );
        let rising = samples.windows(2).filter(|w| w[1] > w[0] + 0.01).count();
        assert!(
            rising >= 5,
            "it passes through intermediate positions: {samples:?}"
        );
        assert!(
            samples.windows(2).all(|w| w[1] >= w[0] - 0.01),
            "monotonic: {samples:?}"
        );
        assert_eq!(
            samples[samples.len() - 1],
            samples[samples.len() - 2],
            "and comes to rest"
        );
    }

    #[test]
    fn right_arrow_switches_off_at_the_end_and_left_at_the_start() {
        let mut c = Carousel::new(24, 900.0);
        assert!(
            !c.info.can_prev && c.info.can_next,
            "at the start only 'next' works"
        );
        let before = c.info.offset;
        let prev = c.info.prev.unwrap().center();
        c.click(prev);
        c.settle();
        assert_eq!(
            c.info.offset, before,
            "clicking a disabled left arrow does nothing"
        );

        for _ in 0..12 {
            if !c.info.can_next {
                break;
            }
            let next = c.info.next.unwrap().center();
            c.click(next);
            c.settle();
        }
        assert!(!c.info.can_next, "reached the end: {:?}", c.info);
        assert!(
            (c.info.offset - c.info.max).abs() < 1.0,
            "exactly at the end: {:?}",
            c.info
        );
        assert!(c.info.can_prev);
        let end = c.info.offset;
        let next = c.info.next.unwrap().center();
        c.click(next);
        c.settle();
        assert_eq!(
            c.info.offset, end,
            "clicking the disabled right arrow does nothing"
        );

        let prev = c.info.prev.unwrap().center();
        c.click(prev);
        c.settle();
        assert!(c.info.offset < end - 100.0, "left works again from the end");
    }

    #[test]
    fn rows_that_fit_have_no_arrows() {
        let mut c = Carousel::new(3, 900.0);
        c.settle();
        assert!(c.info.max < 1.0, "{:?}", c.info);
        assert!(
            c.info.prev.is_none() && c.info.next.is_none(),
            "nothing to scroll, so no arrows"
        );
    }
}
