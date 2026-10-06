//! Our own window title bar, used on Linux where the system draws none (GNOME/Wayland).
//!
//! Provides the bar (drag to move, double-click to maximise, minimise / maximise / close
//! buttons) and the invisible resize handles along the window edges.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, Context, CornerRadius, CursorIcon, FontId, PointerButton,
    Pos2, Rect, ResizeDirection, Sense, Ui, ViewportCommand,
};
use egui_phosphor::regular as icon;

use crate::theme::{self, c_border, c_surface_hover, c_text, c_text_dim, ACCENT};

pub const HEIGHT: f32 = 38.0;
/// Thickness of the invisible resize handles.
pub const EDGE: f32 = 6.0;
const BUTTON_W: f32 = 48.0;

/// Whether the app draws its own title bar. Linux only; `TUNEBOX_NATIVE_TITLEBAR=1` opts out.
pub fn enabled() -> bool {
    cfg!(target_os = "linux")
        && !std::env::var("TUNEBOX_NATIVE_TITLEBAR").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
}

impl Action {
    pub fn command(self, maximized: bool) -> ViewportCommand {
        match self {
            Self::Minimize => ViewportCommand::Minimized(true),
            Self::ToggleMaximize => ViewportCommand::Maximized(!maximized),
            Self::Close => ViewportCommand::Close,
            Self::StartDrag => ViewportCommand::StartDrag,
        }
    }
}

/// Where the parts ended up (exposed for tests).
#[derive(Debug, Clone, Copy)]
pub struct Parts {
    pub minimize: Rect,
    pub maximize: Rect,
    pub close: Rect,
    pub drag: Rect,
}

/// The layout of the bar inside `rect`: three buttons on the right, the rest is the drag area.
pub fn layout(rect: Rect) -> Parts {
    let button = |n: f32| {
        Rect::from_min_size(
            pos2(rect.right() - BUTTON_W * n, rect.top()),
            vec2(BUTTON_W, rect.height()),
        )
    };
    Parts {
        minimize: button(3.0),
        maximize: button(2.0),
        close: button(1.0),
        drag: Rect::from_min_max(rect.min, pos2(rect.right() - BUTTON_W * 3.0, rect.bottom())),
    }
}

fn window_button(ui: &mut Ui, rect: Rect, glyph: &str, tooltip: &str, danger: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(tooltip), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        let bg = if danger { ACCENT } else { c_surface_hover() };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    }
    let fg = if hovered && danger {
        Color32::WHITE
    } else if hovered {
        c_text()
    } else {
        c_text_dim()
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(16.0, theme::icons()),
        fg,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    resp.on_hover_cursor(CursorIcon::Default).clicked()
}

/// Draws the bar into `ui` (which should be exactly [`HEIGHT`] tall) and reports what the user did.
pub fn show(ui: &mut Ui, title: &str, maximized: bool) -> Option<Action> {
    let rect = ui.max_rect();
    let parts = layout(rect);
    let mut action = None;

    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, theme::c_bg());
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, c_border()),
    );

    // Move / maximise: the area left of the buttons.
    let drag = ui.interact(
        parts.drag,
        ui.id().with("titlebar_drag"),
        Sense::click_and_drag(),
    );
    if drag.double_clicked() {
        action = Some(Action::ToggleMaximize);
    } else if drag.drag_started_by(PointerButton::Primary) {
        action = Some(Action::StartDrag);
    }

    // Icon + title.
    let logo = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(18.0, 18.0));
    ui.put(
        logo,
        egui::Image::new(egui::include_image!("../../assets/icons/32x32.png"))
            .fit_to_exact_size(logo.size())
            .show_loading_spinner(false),
    );
    ui.painter().text(
        pos2(rect.left() + 40.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::new(13.0, theme::bold_family()),
        c_text_dim(),
    );

    if window_button(
        ui,
        parts.minimize,
        icon::MINUS,
        crate::i18n::t("Minimize"),
        false,
    ) {
        action = Some(Action::Minimize);
    }
    let (glyph, tip) = if maximized {
        (icon::COPY_SIMPLE, crate::i18n::t("Restore"))
    } else {
        (icon::SQUARE, crate::i18n::t("Maximize"))
    };
    if window_button(ui, parts.maximize, glyph, tip, false) {
        action = Some(Action::ToggleMaximize);
    }
    if window_button(ui, parts.close, icon::X, crate::i18n::t("Close"), true) {
        action = Some(Action::Close);
    }
    action
}

/// Which edge/corner of `rect` the pointer is on (within `edge` px), if any.
/// Corners get a larger grab area (2×`edge` along the edge) so they are easy to hit.
pub fn resize_direction(pos: Pos2, rect: Rect, edge: f32) -> Option<ResizeDirection> {
    if !rect.contains(pos) {
        return None;
    }
    let (dl, dr, dt, db) = (
        pos.x - rect.left(),
        rect.right() - pos.x,
        pos.y - rect.top(),
        rect.bottom() - pos.y,
    );
    let corner = edge * 2.0;
    let (l, r, t, b) = (dl < edge, dr < edge, dt < edge, db < edge);
    let (lc, rc, tc, bc) = (dl < corner, dr < corner, dt < corner, db < corner);
    Some(match () {
        _ if (t && lc) || (l && tc) => ResizeDirection::NorthWest,
        _ if (t && rc) || (r && tc) => ResizeDirection::NorthEast,
        _ if (b && lc) || (l && bc) => ResizeDirection::SouthWest,
        _ if (b && rc) || (r && bc) => ResizeDirection::SouthEast,
        _ if t => ResizeDirection::North,
        _ if b => ResizeDirection::South,
        _ if l => ResizeDirection::West,
        _ if r => ResizeDirection::East,
        _ => return None,
    })
}

fn cursor_for(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// Edge handles + a thin outline (a borderless window has no frame to tell where it ends).
/// Call once per frame, after the panels.
pub fn window_chrome(ctx: &Context, maximized: bool) {
    let rect = ctx.content_rect();
    if maximized || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("window_outline"),
    ));
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        egui::Stroke::new(1.0, c_border()),
        egui::StrokeKind::Inside,
    );
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if let Some(dir) = resize_direction(pos, rect, EDGE) {
        ctx.set_cursor_icon(cursor_for(dir));
        if ctx.input(|i| i.pointer.button_pressed(PointerButton::Primary)) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, RawInput};

    fn rect() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0))
    }

    #[test]
    fn resize_zones() {
        let r = rect();
        let d = |x, y| resize_direction(pos2(x, y), r, 6.0);
        assert_eq!(
            d(500.0, 350.0),
            None,
            "the middle of the window is not a handle"
        );
        assert_eq!(d(500.0, 2.0), Some(ResizeDirection::North));
        assert_eq!(d(500.0, 698.0), Some(ResizeDirection::South));
        assert_eq!(d(2.0, 350.0), Some(ResizeDirection::West));
        assert_eq!(d(998.0, 350.0), Some(ResizeDirection::East));
        assert_eq!(d(2.0, 2.0), Some(ResizeDirection::NorthWest));
        assert_eq!(d(997.0, 3.0), Some(ResizeDirection::NorthEast));
        assert_eq!(d(3.0, 697.0), Some(ResizeDirection::SouthWest));
        assert_eq!(d(997.0, 697.0), Some(ResizeDirection::SouthEast));
        // corners are easier to grab than plain edges
        assert_eq!(d(10.0, 2.0), Some(ResizeDirection::NorthWest));
        assert_eq!(d(30.0, 2.0), Some(ResizeDirection::North));
        assert_eq!(d(-1.0, 5.0), None, "outside the window");
        assert_eq!(d(7.0, 350.0), None, "just inside the handle");
    }

    #[test]
    fn layout_puts_three_buttons_on_the_right_and_leaves_the_rest_to_drag() {
        let p = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, HEIGHT)));
        assert_eq!(p.close.right(), 800.0);
        assert_eq!(p.maximize.right(), p.close.left());
        assert_eq!(p.minimize.right(), p.maximize.left());
        assert_eq!(p.drag.right(), p.minimize.left());
        assert!(p.drag.width() > 500.0);
    }

    /// Drives `show` with synthetic input.
    struct Harness {
        ctx: Context,
        maximized: bool,
        parts: Option<Parts>,
        actions: Vec<Action>,
    }

    impl Harness {
        fn new(maximized: bool) -> Self {
            let mut h = Self {
                ctx: Context::default(),
                maximized,
                parts: None,
                actions: vec![],
            };
            crate::theme::install(&h.ctx);
            egui_extras::install_image_loaders(&h.ctx);
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 300.0))),
                events,
                ..RawInput::default()
            };
            let (maximized, parts, actions) = (self.maximized, &mut self.parts, &mut self.actions);
            let mut out = self.ctx.run_ui(input, |ui| {
                ui.set_max_height(HEIGHT);
                *parts = Some(layout(ui.max_rect()));
                actions.extend(show(ui, "Tunebox", maximized));
            });
            out.textures_delta.clear();
        }

        fn parts(&self) -> Parts {
            self.parts.expect("laid out")
        }

        fn button(&mut self, at: Pos2, pressed: bool) {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }]);
        }

        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.button(at, true);
            self.button(at, false);
            self.frame(vec![]);
        }
    }

    #[test]
    fn buttons_emit_their_actions() {
        for (pick, expected) in [
            (0, Action::Minimize),
            (1, Action::ToggleMaximize),
            (2, Action::Close),
        ] {
            let mut h = Harness::new(false);
            let p = h.parts();
            let at = [p.minimize, p.maximize, p.close][pick].center();
            h.click(at);
            assert_eq!(h.actions, vec![expected], "button {pick}");
        }
    }

    #[test]
    fn dragging_the_empty_bar_starts_a_window_move_and_double_click_maximises() {
        let mut h = Harness::new(false);
        let from = h.parts().drag.center();
        h.frame(vec![Event::PointerMoved(from)]);
        h.button(from, true);
        for step in 1..=5 {
            h.frame(vec![Event::PointerMoved(
                from + vec2(step as f32 * 6.0, 0.0),
            )]);
        }
        assert!(h.actions.contains(&Action::StartDrag), "{:?}", h.actions);
        assert!(!h.actions.contains(&Action::Close));

        let mut h = Harness::new(false);
        let at = h.parts().drag.center();
        h.click(at);
        assert!(
            h.actions.is_empty(),
            "a single click on the bar does nothing: {:?}",
            h.actions
        );
        h.click(at);
        assert!(
            h.actions.contains(&Action::ToggleMaximize),
            "{:?}",
            h.actions
        );
    }

    #[test]
    fn maximize_action_maps_to_the_right_viewport_command() {
        assert!(matches!(
            Action::ToggleMaximize.command(false),
            ViewportCommand::Maximized(true)
        ));
        assert!(matches!(
            Action::ToggleMaximize.command(true),
            ViewportCommand::Maximized(false)
        ));
        assert!(matches!(
            Action::Close.command(false),
            ViewportCommand::Close
        ));
        assert!(matches!(
            Action::StartDrag.command(false),
            ViewportCommand::StartDrag
        ));
    }
}
