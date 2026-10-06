//! All visual styling lives here: palette, `Visuals`, `Style`, text styles, fonts.

use eframe::egui::{
    self, epaint::Shadow, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Margin, Stroke, TextStyle, Vec2,
};

/// The colours that differ between the dark and light themes.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    /// Surfaces that float above the page (drawers, popups): they must read as raised.
    pub elevated: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    /// Fill of the "primary" pills (Play, active split button) and the text on them.
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_hover: Color32,
    /// Hover fill of secondary pills, and the unfilled part of sliders.
    pub strong: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0F, 0x0F, 0x0F),
    surface: Color32::from_rgb(0x18, 0x18, 0x18),
    elevated: Color32::from_rgb(0x20, 0x20, 0x22),
    surface_hover: Color32::from_rgb(0x24, 0x24, 0x24),
    surface_active: Color32::from_rgb(0x2E, 0x2E, 0x2E),
    border: Color32::from_rgb(0x2A, 0x2A, 0x2A),
    text: Color32::from_rgb(0xF5, 0xF5, 0xF5),
    text_dim: Color32::from_rgb(0xAA, 0xAA, 0xAA),
    text_faint: Color32::from_rgb(0x70, 0x70, 0x70),
    primary: Color32::from_gray(0xF1),
    on_primary: Color32::from_rgb(0x0F, 0x0F, 0x0F),
    primary_hover: Color32::WHITE,
    strong: Color32::from_gray(0x3A),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF6, 0xF6, 0xF7),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    elevated: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    surface_hover: Color32::from_rgb(0xEC, 0xEC, 0xEE),
    surface_active: Color32::from_rgb(0xDE, 0xDE, 0xE2),
    border: Color32::from_rgb(0xD8, 0xD8, 0xDC),
    text: Color32::from_rgb(0x14, 0x14, 0x16),
    text_dim: Color32::from_rgb(0x55, 0x55, 0x5C),
    text_faint: Color32::from_rgb(0x8E, 0x8E, 0x96),
    primary: Color32::from_rgb(0x14, 0x14, 0x16),
    on_primary: Color32::WHITE,
    primary_hover: Color32::from_rgb(0x34, 0x34, 0x3A),
    strong: Color32::from_rgb(0xCC, 0xCC, 0xD2),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    pub fn from_code(code: &str) -> Mode {
        if code.trim().eq_ignore_ascii_case("light") {
            Mode::Light
        } else {
            Mode::Dark
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }
}

static LIGHT_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn mode() -> Mode {
    if LIGHT_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        Mode::Light
    } else {
        Mode::Dark
    }
}

pub fn palette() -> &'static Palette {
    match mode() {
        Mode::Dark => &DARK,
        Mode::Light => &LIGHT,
    }
}

pub fn c_bg() -> Color32 {
    palette().bg
}
pub fn c_surface() -> Color32 {
    palette().surface
}
pub fn c_elevated() -> Color32 {
    palette().elevated
}
pub fn c_surface_hover() -> Color32 {
    palette().surface_hover
}
pub fn c_surface_active() -> Color32 {
    palette().surface_active
}
pub fn c_border() -> Color32 {
    palette().border
}
pub fn c_text() -> Color32 {
    palette().text
}
pub fn c_text_dim() -> Color32 {
    palette().text_dim
}
pub fn c_text_faint() -> Color32 {
    palette().text_faint
}
pub fn c_primary() -> Color32 {
    palette().primary
}
pub fn c_primary_hover() -> Color32 {
    palette().primary_hover
}
pub fn c_strong() -> Color32 {
    palette().strong
}
pub fn c_on_primary() -> Color32 {
    palette().on_primary
}
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x00, 0x33);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xFF, 0x33, 0x5C);

pub const ART_RADIUS: u8 = 12;
pub const CARD_RADIUS: u8 = 14;

/// Text style for section titles.
pub fn title_style() -> TextStyle {
    TextStyle::Name("Title".into())
}

/// Text style for small captions.
pub fn caption_style() -> TextStyle {
    TextStyle::Name("Caption".into())
}

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn fill_icons() -> FontFamily {
    FontFamily::Name("icons-fill".into())
}

/// Icon-only family: Phosphor first, so Inter can never shadow icon codepoints.
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

pub fn bold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(bold_family())
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // egui keeps one style per theme; both must carry our text styles.
    ctx.set_style_of(egui::Theme::Dark, style());
    ctx.set_style_of(egui::Theme::Light, style());
    set_mode(ctx, mode());
}

/// Switch between the dark and light palette and restyle the whole UI.
pub fn set_mode(ctx: &egui::Context, mode: Mode) {
    LIGHT_MODE.store(mode == Mode::Light, std::sync::atomic::Ordering::Relaxed);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.set_visuals_of(egui::Theme::Light, visuals());
    ctx.options_mut(|o| {
        // The app chooses its own theme; never follow the OS.
        o.theme_preference = match mode {
            Mode::Dark => egui::ThemePreference::Dark,
            Mode::Light => egui::ThemePreference::Light,
        };
    });
    ctx.request_repaint();
}

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-bold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Bold.ttf")).into(),
    );
    // Inter first, egui's built-in fonts stay as fallback for missing glyphs (CJK/emoji).
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    fonts
        .families
        .insert(bold_family(), vec!["inter-bold".into(), "inter".into()]);

    // Inter has no Chinese/Japanese/Korean glyphs; borrow one from the system when there is one.
    if let Some(data) = system_cjk_font() {
        fonts.font_data.insert("cjk".into(), data.into());
        for family in [FontFamily::Proportional, bold_family()] {
            fonts.families.entry(family).or_default().push("cjk".into());
        }
    }

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    egui_phosphor::add_font_bytes_as_family(
        &mut fonts,
        "phosphor-fill",
        egui_phosphor::Variant::Fill.font_bytes(),
    );
    fonts
        .families
        .insert(icons(), vec!["phosphor".into(), "inter".into()]);
    fonts
        .families
        .insert(fill_icons(), vec!["phosphor-fill".into(), "inter".into()]);
    fonts
}

/// A Simplified-Chinese capable font from the operating system (not bundled: they are 10+ MB).
fn system_cjk_font() -> Option<FontData> {
    // (path, face index inside a .ttc collection)
    const CANDIDATES: &[(&str, u32)] = &[
        // Linux: Noto Sans CJK ships as a collection ordered JP, KR, SC, TC, HK.
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2),
        ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 2),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
        (
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            0,
        ),
        // macOS
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
        // Windows
        ("C:\\Windows\\Fonts\\msyh.ttc", 0),
        ("C:\\Windows\\Fonts\\simsun.ttc", 0),
    ];
    for (path, index) in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut data = FontData::from_owned(bytes);
            data.index = *index;
            return Some(data);
        }
    }
    tracing::info!("no system CJK font found; Chinese text will not render");
    None
}

pub fn style() -> egui::Style {
    let mut style = egui::Style {
        text_styles: [
            (TextStyle::Heading, FontId::new(30.0, bold_family())),
            (title_style(), FontId::new(20.0, bold_family())),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, bold_family())),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (caption_style(), FontId::new(12.0, FontFamily::Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into(),
        ..egui::Style::default()
    };

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(10.0, 10.0);
    s.button_padding = Vec2::new(16.0, 8.0);
    s.window_margin = Margin::same(16);
    s.interact_size = Vec2::new(40.0, 36.0);
    s.menu_margin = Margin::same(8);
    s.scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: 8.0,
        floating_width: 4.0,
        floating_allocated_width: 0.0,
        foreground_color: false,
        ..egui::style::ScrollStyle::thin()
    };
    style.animation_time = 0.12;
    style
}

pub fn visuals() -> egui::Visuals {
    let dark = mode() == Mode::Dark;
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.dark_mode = dark;
    v.override_text_color = None;
    v.panel_fill = c_bg();
    v.window_fill = c_surface();
    v.extreme_bg_color = if dark {
        Color32::from_rgb(0x1F, 0x1F, 0x1F)
    } else {
        Color32::WHITE
    }; // text edit background
    v.faint_bg_color = c_surface();
    v.code_bg_color = c_surface();
    v.window_stroke = Stroke::new(1.0, c_border());
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.popup_shadow = v.window_shadow;
    v.hyperlink_color = ACCENT_HOVER;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, c_text());
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    let radius = CornerRadius::same(10);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c_surface();
    w.noninteractive.weak_bg_fill = c_surface();
    w.noninteractive.bg_stroke = Stroke::new(1.0, c_border());
    w.noninteractive.fg_stroke = Stroke::new(1.0, c_text_dim());
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = c_surface_hover();
    w.inactive.weak_bg_fill = c_surface_hover();
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, c_text());
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = c_surface_active();
    w.hovered.weak_bg_fill = c_surface_active();
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.5, c_text());
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = ACCENT;
    w.active.weak_bg_fill = ACCENT;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open = w.hovered;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_themes_carry_the_custom_text_styles() {
        let ctx = egui::Context::default();
        install(&ctx);
        for mode in [Mode::Light, Mode::Dark] {
            set_mode(&ctx, mode);
            // a text style missing from the active theme's style panics when drawing a heading
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(egui::RichText::new("x").text_style(title_style()));
                ui.label(egui::RichText::new("x").text_style(caption_style()));
            });
            output.textures_delta.clear(); // no GPU here
        }
        set_mode(&ctx, Mode::Dark);
    }

    #[test]
    fn contrast_of_primary_text_on_background_is_high() {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        let ratio = |a: Color32, b: Color32| {
            let (la, lb) = (lum(a), lum(b));
            (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
        };
        for p in [DARK, LIGHT] {
            assert!(ratio(p.text, p.bg) > 15.0);
            assert!(
                ratio(p.text_dim, p.bg) > 6.5,
                "secondary text must be readable"
            );
            assert!(ratio(p.text_dim, p.surface) > 6.0);
            assert!(ratio(p.on_primary, p.primary) > 10.0);
        }
        assert!(
            ratio(Color32::WHITE, ACCENT) > 3.5,
            "text on accent buttons"
        );
    }
}
