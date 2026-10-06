//! System tray icon with Show / play-pause / next / previous / Quit.
//!
//! Linux uses the StatusNotifierItem protocol (`ksni`, plain D-Bus, no GTK); macOS and Windows use
//! `tray-icon`. Menu clicks are turned into the same [`Action`]s the UI sends, or into viewport
//! commands (`egui::Context` can be used from any thread).
//!
//! The icon may fail to appear (GNOME without the AppIndicator extension has no tray host), so
//! "close to tray" only hides the window while [`Tray::active`] is true.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, ViewportCommand};
use tokio::sync::mpsc::UnboundedSender;
use ytm_player::Command;

use crate::backend::Action;
use crate::i18n::t;

/// What the tray menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Show,
    PlayPause,
    Next,
    Prev,
    Quit,
}

#[derive(Default)]
struct Shared {
    /// An icon is actually showing.
    active: AtomicBool,
    /// The user picked Quit: the window's close must not be turned into "hide".
    quitting: AtomicBool,
    /// The window was hidden to the tray and not shown again since.
    hidden: AtomicBool,
}

/// Runs menu items; cheap to clone into callbacks on other threads.
#[derive(Clone)]
struct Dispatcher {
    ctx: egui::Context,
    actions: UnboundedSender<Action>,
    shared: Arc<Shared>,
}

impl Dispatcher {
    fn run(&self, item: Item) {
        match item {
            Item::Show => show_window(&self.ctx, &self.shared),
            Item::PlayPause => self.playback(Command::Toggle),
            Item::Next => self.playback(Command::Next),
            Item::Prev => self.playback(Command::Prev),
            Item::Quit => {
                self.shared.quitting.store(true, Ordering::SeqCst);
                self.ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn playback(&self, cmd: Command) {
        let _ = self.actions.send(Action::Playback(cmd));
        self.ctx.request_repaint();
    }
}

/// Wayland compositors ignore "hide window"; minimising is the closest thing.
fn hides_by_minimizing() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn show_window(ctx: &egui::Context, shared: &Shared) {
    shared.hidden.store(false, Ordering::SeqCst);
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
}

/// Window icon scaled for the tray, as straight RGBA (macOS draws its own template instead).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn icon_rgba(size: u32) -> Option<(Vec<u8>, u32)> {
    let img = image::load_from_memory(include_bytes!("../assets/icons/256x256.png")).ok()?;
    let img = image::imageops::resize(
        &img.into_rgba8(),
        size,
        size,
        image::imageops::FilterType::Lanczos3,
    );
    Some((img.into_raw(), size))
}

/// What the tray shows besides its menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayState {
    /// "Title — Artist" of the current song (tooltip), empty when nothing is loaded.
    pub line: String,
    /// Text next to the icon; `None` hides it.
    pub label: Option<String>,
    /// A play badge is drawn on the icon.
    pub playing: bool,
}

/// Longest label shown next to the icon (the panel is shared with other indicators).
const LABEL_MAX_CHARS: usize = 32;

/// Truncates to [`LABEL_MAX_CHARS`] characters with an ellipsis.
pub fn shorten_label(text: &str) -> String {
    if text.chars().count() <= LABEL_MAX_CHARS {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(LABEL_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// Draws a small accent-coloured badge with two pause bars in the bottom-right corner of a
/// straight-RGBA square icon (so the tray shows "playing" without any text).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn draw_playing_badge(rgba: &mut [u8], size: u32) {
    let r = size as f32 * 0.24;
    let (cx, cy) = (size as f32 - r - 1.0, size as f32 - r - 1.0);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d > r {
                continue;
            }
            let bar = dx.abs() > r * 0.18 && dx.abs() < r * 0.55 && dy.abs() < r * 0.55;
            let color: [u8; 4] = if bar {
                [255, 255, 255, 255]
            } else {
                [29, 185, 84, 255]
            };
            let i = ((y * size + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&color);
        }
    }
}

/// Small vector glyphs rasterised in code (no asset files): menu icons and the macOS menu-bar
/// template. Only Windows and macOS draw them (the template only macOS), but the maths is unit-tested everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod glyphs {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Glyph {
        Play,
        Pause,
        Next,
        Prev,
    }

    /// Is the point inside the glyph? Coordinates are in the unit square, y down.
    fn inside(g: Glyph, x: f32, y: f32) -> bool {
        match g {
            Glyph::Play => (0.2..=0.88).contains(&x) && (y - 0.5).abs() <= (0.88 - x) / 0.68 * 0.38,
            Glyph::Pause => {
                (0.15..=0.85).contains(&y)
                    && ((0.24..=0.42).contains(&x) || (0.58..=0.76).contains(&x))
            }
            Glyph::Next => {
                ((0.12..=0.68).contains(&x) && (y - 0.5).abs() <= (0.68 - x) / 0.56 * 0.36)
                    || ((0.74..=0.88).contains(&x) && (0.14..=0.86).contains(&y))
            }
            Glyph::Prev => inside(Glyph::Next, 1.0 - x, y),
        }
    }

    /// Fraction (0..1) of the pixel `(px, py)` of a `size`-square image covered by `f`, by 4x4
    /// supersampling.
    fn coverage(px: u32, py: u32, size: u32, f: impl Fn(f32, f32) -> bool) -> f32 {
        let mut hit = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / size as f32;
                let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / size as f32;
                hit += f(x, y) as u32;
            }
        }
        hit as f32 / 16.0
    }

    /// Straight RGBA of one glyph in a single colour.
    pub fn glyph_rgba(g: Glyph, size: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let a = coverage(x, y, size, |u, v| inside(g, u, v));
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8]);
            }
        }
        out
    }

    /// macOS menu-bar template: a black disc with the state cut out: play while playing, pause
    /// otherwise (the icon shows what is happening, not what a click would do). Only the alpha
    /// channel matters; the system tints it for light and dark menu bars.
    pub fn template_rgba(playing: bool, size: u32) -> Vec<u8> {
        let g = if playing { Glyph::Play } else { Glyph::Pause };
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let disc = coverage(x, y, size, |u, v| {
                    (u - 0.5).powi(2) + (v - 0.5).powi(2) <= 0.46 * 0.46
                });
                // The glyph fills 60% of the disc's diameter, centred.
                let cut = coverage(x, y, size, |u, v| {
                    inside(g, (u - 0.5) / 0.6 + 0.5 + 0.02, (v - 0.5) / 0.6 + 0.5)
                });
                let a = (disc * (1.0 - cut) * 255.0).round() as u8;
                out.extend_from_slice(&[0, 0, 0, a]);
            }
        }
        out
    }
}

pub struct Tray {
    shared: Arc<Shared>,
    #[allow(dead_code)] // keeps the icon alive (dropping it removes it)
    platform: Option<platform::Platform>,
}

impl Tray {
    /// Starts the tray icon when `enabled`. Never fails: without a tray host the app just runs
    /// without one.
    pub fn start(
        rt: &tokio::runtime::Handle,
        ctx: egui::Context,
        actions: UnboundedSender<Action>,
        enabled: bool,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let platform = enabled.then(|| {
            platform::start(
                rt,
                Dispatcher {
                    ctx,
                    actions,
                    shared: shared.clone(),
                },
            )
        });
        Self {
            shared,
            platform: platform.flatten(),
        }
    }

    /// The icon is showing, so hiding the window cannot strand the user.
    pub fn active(&self) -> bool {
        self.shared.active.load(Ordering::SeqCst)
    }

    pub fn quitting(&self) -> bool {
        self.shared.quitting.load(Ordering::SeqCst)
    }

    /// The window was hidden to the tray (a hidden window may still report itself as focused).
    pub fn hidden(&self) -> bool {
        self.shared.hidden.load(Ordering::SeqCst)
    }

    /// Another launch brought the window forward.
    pub fn mark_shown(&self) {
        self.shared.hidden.store(false, Ordering::SeqCst);
    }

    pub fn hide_window(&self, ctx: &egui::Context) {
        self.shared.hidden.store(true, Ordering::SeqCst);
        ctx.send_viewport_cmd(if hides_by_minimizing() {
            ViewportCommand::Minimized(true)
        } else {
            ViewportCommand::Visible(false)
        });
    }

    /// macOS: a window hidden to the tray must not leave a dock icon behind (clicking it cannot
    /// bring the window back), so the app turns into a menu-bar-only one while hidden. Call on the
    /// main thread; cheap when nothing changed.
    pub fn sync_dock(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2::MainThreadMarker;
            use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy as Policy};
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let want = if self.hidden() {
                Policy::Accessory
            } else {
                Policy::Regular
            };
            let app = NSApplication::sharedApplication(mtm);
            if app.activationPolicy() != want {
                app.setActivationPolicy(want);
            }
        }
    }

    /// Pushes the tooltip, label and badge; the platform ignores what it cannot show.
    pub fn set_state(&self, state: &TrayState) {
        if let Some(p) = &self.platform {
            p.set_state(state);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use ksni::menu::StandardItem;
    use ksni::{MenuItem, TrayMethods};

    use super::{draw_playing_badge, icon_rgba, t, Dispatcher, Item, TrayState};

    /// StatusNotifierItem wants ARGB, network byte order.
    fn argb_icon(mut rgba: Vec<u8>, size: u32, playing: bool) -> Vec<ksni::Icon> {
        if playing {
            draw_playing_badge(&mut rgba, size);
        }
        for px in rgba.as_chunks_mut::<4>().0 {
            px.rotate_right(1);
        }
        vec![ksni::Icon {
            width: size as i32,
            height: size as i32,
            data: rgba,
        }]
    }

    struct LinuxTray {
        state: TrayState,
        base: Vec<u8>,
        size: u32,
        icon: Vec<ksni::Icon>,
        dispatcher: Dispatcher,
    }

    impl ksni::Tray for LinuxTray {
        fn id(&self) -> String {
            "tunebox".into()
        }

        fn title(&self) -> String {
            if self.state.line.is_empty() {
                "Tunebox".into()
            } else {
                self.state.line.clone()
            }
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Tunebox".into(),
                description: self.state.line.clone(),
                ..Default::default()
            }
        }

        fn ayatana_label(&self) -> String {
            self.state.label.clone().unwrap_or_default()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            self.dispatcher.run(Item::Show);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let item = |label: &str, item: Item| -> MenuItem<Self> {
                // Freedesktop theme icons; hosts without them just show the label.
                let icon_name = match item {
                    Item::PlayPause => "media-playback-start-symbolic",
                    Item::Next => "media-skip-forward-symbolic",
                    Item::Prev => "media-skip-backward-symbolic",
                    Item::Show | Item::Quit => "",
                };
                StandardItem {
                    label: label.into(),
                    icon_name: icon_name.into(),
                    activate: Box::new(move |tray: &mut LinuxTray| tray.dispatcher.run(item)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item(t("Show Tunebox"), Item::Show),
                MenuItem::Separator,
                item(t("Play / pause"), Item::PlayPause),
                item(t("Next track"), Item::Next),
                item(t("Previous track"), Item::Prev),
                MenuItem::Separator,
                item(t("Quit"), Item::Quit),
            ]
        }
    }

    pub struct Platform {
        rt: tokio::runtime::Handle,
        handle: Arc<tokio::sync::Mutex<Option<ksni::Handle<LinuxTray>>>>,
    }

    pub fn start(rt: &tokio::runtime::Handle, dispatcher: Dispatcher) -> Option<Platform> {
        let (base, size) = icon_rgba(64)?;
        let tray = LinuxTray {
            state: TrayState::default(),
            icon: argb_icon(base.clone(), size, false),
            base,
            size,
            dispatcher: dispatcher.clone(),
        };
        let handle = Arc::new(tokio::sync::Mutex::new(None));
        let slot = handle.clone();
        rt.spawn(async move {
            match tray.spawn().await {
                Ok(h) => {
                    *slot.lock().await = Some(h);
                    dispatcher.shared.active.store(true, Ordering::SeqCst);
                    tracing::info!("tray icon registered");
                }
                Err(e) => tracing::warn!(error = %e, "no tray icon (closing the window will quit)"),
            }
        });
        Some(Platform {
            rt: rt.clone(),
            handle,
        })
    }

    impl Platform {
        pub fn set_state(&self, state: &TrayState) {
            let (handle, state) = (self.handle.clone(), state.clone());
            self.rt.spawn(async move {
                if let Some(h) = handle.lock().await.as_ref() {
                    h.update(|tray| {
                        if tray.state.playing != state.playing {
                            tray.icon = argb_icon(tray.base.clone(), tray.size, state.playing);
                        }
                        tray.state = state;
                    })
                    .await;
                }
            });
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use std::cell::Cell;
    use std::sync::atomic::Ordering;

    use tray_icon::menu::{
        IconMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem,
    };
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::glyphs::{self, Glyph};
    #[cfg(not(target_os = "macos"))]
    use super::{draw_playing_badge, icon_rgba};
    use super::{t, Dispatcher, Item, TrayState};

    /// Menu glyph colour on Windows: readable on both the light and the dark menu. (macOS draws
    /// the menu glyphs as template images and tints them itself.)
    #[cfg(not(target_os = "macos"))]
    const MENU_GLYPH_RGB: [u8; 3] = [128, 128, 128];
    const MENU_GLYPH_PX: u32 = 32;
    /// Menu-bar icon size (22 pt at 2x).
    #[cfg(target_os = "macos")]
    const BAR_ICON_PX: u32 = 44;
    #[cfg(not(target_os = "macos"))]
    const TRAY_ICON_PX: u32 = 64;

    /// Must be created on the main thread, which `eframe`'s app creator is.
    pub struct Platform {
        icon: TrayIcon,
        playing: Cell<bool>,
    }

    fn menu_item(item: Item, glyph: Option<Glyph>) -> Box<dyn IsMenuItem> {
        let label = match item {
            Item::Show => t("Show Tunebox"),
            Item::PlayPause => t("Play / pause"),
            Item::Next => t("Next track"),
            Item::Prev => t("Previous track"),
            Item::Quit => t("Quit"),
        };
        let Some(glyph) = glyph else {
            return Box::new(MenuItem::new(label, true, None));
        };
        let entry = IconMenuItem::new(label, true, None, None);
        #[cfg(target_os = "macos")]
        let rgba = glyphs::glyph_rgba(glyph, MENU_GLYPH_PX, [0, 0, 0]);
        #[cfg(not(target_os = "macos"))]
        let rgba = glyphs::glyph_rgba(glyph, MENU_GLYPH_PX, MENU_GLYPH_RGB);
        if let Ok(icon) = tray_icon::menu::Icon::from_rgba(rgba, MENU_GLYPH_PX, MENU_GLYPH_PX) {
            #[cfg(target_os = "macos")]
            entry.set_icon_templated(Some(icon));
            #[cfg(not(target_os = "macos"))]
            entry.set_icon(Some(icon));
        }
        Box::new(entry)
    }

    /// The icon for the current play state: a monochrome template on macOS (the menu bar is
    /// tinted by the system), the app icon with a badge while playing elsewhere.
    fn tray_icon_for(playing: bool) -> Option<tray_icon::Icon> {
        #[cfg(target_os = "macos")]
        {
            let rgba = glyphs::template_rgba(playing, BAR_ICON_PX);
            tray_icon::Icon::from_rgba(rgba, BAR_ICON_PX, BAR_ICON_PX).ok()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let (mut rgba, size) = icon_rgba(TRAY_ICON_PX)?;
            if playing {
                draw_playing_badge(&mut rgba, size);
            }
            tray_icon::Icon::from_rgba(rgba, size, size).ok()
        }
    }

    pub fn start(_rt: &tokio::runtime::Handle, dispatcher: Dispatcher) -> Option<Platform> {
        let items: [(Item, Box<dyn IsMenuItem>); 5] = [
            (Item::Show, menu_item(Item::Show, None)),
            (
                Item::PlayPause,
                menu_item(Item::PlayPause, Some(Glyph::Play)),
            ),
            (Item::Next, menu_item(Item::Next, Some(Glyph::Next))),
            (Item::Prev, menu_item(Item::Prev, Some(Glyph::Prev))),
            (Item::Quit, menu_item(Item::Quit, None)),
        ];
        let menu = Menu::new();
        for (i, (_, item)) in items.iter().enumerate() {
            if i == 1 || i == 4 {
                menu.append(&PredefinedMenuItem::separator()).ok()?;
            }
            menu.append(item.as_ref()).ok()?;
        }
        let ids: Vec<_> = items.iter().map(|(it, m)| (*it, m.id().clone())).collect();
        let d = dispatcher.clone();
        MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
            if let Some((item, _)) = ids.iter().find(|(_, id)| *id == ev.id) {
                d.run(*item);
            }
        }));
        let d = dispatcher.clone();
        TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                d.run(Item::Show);
            }
        }));
        let icon = tray_icon_for(false)?;
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Tunebox");
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon);
        match builder.build() {
            Ok(icon) => {
                dispatcher.shared.active.store(true, Ordering::SeqCst);
                Some(Platform {
                    icon,
                    playing: Cell::new(false),
                })
            }
            Err(e) => {
                tracing::warn!(error = %e, "no tray icon");
                None
            }
        }
    }

    impl Platform {
        pub fn set_state(&self, state: &TrayState) {
            let text = if state.line.is_empty() {
                "Tunebox"
            } else {
                &state.line
            };
            let _ = self.icon.set_tooltip(Some(text));
            if self.playing.replace(state.playing) != state.playing {
                if let Some(icon) = tray_icon_for(state.playing) {
                    #[cfg(target_os = "macos")]
                    let _ = self.icon.set_icon_templated(Some(icon));
                    #[cfg(not(target_os = "macos"))]
                    let _ = self.icon.set_icon(Some(icon));
                }
            }
            // Text beside the icon exists in the macOS menu bar only (Windows has none).
            #[cfg(target_os = "macos")]
            self.icon.set_title(state.label.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::glyphs::*;
    use super::*;

    fn alpha(rgba: &[u8], size: u32, x: u32, y: u32) -> u8 {
        rgba[((y * size + x) * 4 + 3) as usize]
    }

    #[test]
    fn glyphs_are_drawn_inside_their_square() {
        for g in [Glyph::Play, Glyph::Pause, Glyph::Next, Glyph::Prev] {
            let px = glyph_rgba(g, 32, [1, 2, 3]);
            assert_eq!(px.len(), 32 * 32 * 4);
            assert!(px.chunks(4).any(|p| p[3] == 255), "{g:?} has solid pixels");
            assert_eq!(alpha(&px, 32, 0, 0), 0, "{g:?} leaves the corner empty");
            assert_eq!(&px[..3], &[1, 2, 3]);
        }
    }

    #[test]
    fn prev_mirrors_next() {
        let (n, p) = (
            glyph_rgba(Glyph::Next, 32, [0; 3]),
            glyph_rgba(Glyph::Prev, 32, [0; 3]),
        );
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(alpha(&n, 32, x, y), alpha(&p, 32, 31 - x, y));
            }
        }
    }

    #[test]
    fn template_is_a_disc_with_a_hole_that_changes_with_state() {
        let (idle, playing) = (template_rgba(false, 44), template_rgba(true, 44));
        assert_eq!(alpha(&idle, 44, 0, 0), 0, "corner outside the disc");
        assert_eq!(alpha(&idle, 44, 22, 3), 255, "rim of the disc is solid");
        assert_eq!(alpha(&playing, 44, 20, 22), 0, "play glyph is cut out");
        assert_ne!(idle, playing);
    }

    #[test]
    fn label_is_shortened_on_char_boundaries() {
        assert_eq!(shorten_label("short"), "short");
        let long = "é".repeat(40);
        let out = shorten_label(&long);
        assert_eq!(out.chars().count(), LABEL_MAX_CHARS);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn badge_touches_only_the_corner() {
        let mut px = vec![0u8; 64 * 64 * 4];
        draw_playing_badge(&mut px, 64);
        assert_eq!(px[3], 0, "top-left stays transparent");
        let last = (64 * 64 - 1) * 4;
        assert_eq!(
            px[last + 3],
            0,
            "outermost corner pixel is outside the circle"
        );
        assert!(px.chunks(4).any(|p| p[3] == 255));
    }
}
