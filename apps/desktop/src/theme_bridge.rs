//! Projects the versioned Perkolator theme onto GPUI Kit's theme, and keeps
//! the extra Perkolator roles (hairline, tertiary text, focus) in a global.
//! This is the only place in the app that converts raw theme colours.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, Pixels, px, rgba};
use perkolator_theme::{Rgba, ThemeFile};

/// Perkolator-specific semantic roles not present in GPUI Kit's theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub hairline: Hsla,
    pub tertiary: Hsla,
    pub focus: Hsla,
    pub selection: Hsla,
    pub selection_foreground: Hsla,
    pub hover: Hsla,
    pub chrome: Hsla,
    pub raised: Hsla,
    pub warning: Hsla,
    pub destructive: Hsla,
    pub row_height: Pixels,
}

impl Global for Palette {}

impl Palette {
    pub fn get(cx: &App) -> Palette {
        *cx.global::<Palette>()
    }
}

fn color(value: Rgba) -> Hsla {
    rgba(value.to_u32()).into()
}

pub fn apply(cx: &mut App) {
    let file = ThemeFile::builtin_dark();
    let colors = &file.colors;
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        let target = &mut theme.colors;
        target.background = color(colors.background);
        target.foreground = color(colors.foreground);
        target.muted = color(colors.surface);
        target.muted_foreground = color(colors.muted_foreground);
        target.border = color(colors.hairline);
        target.input = color(colors.border);
        target.ring = color(colors.focus);
        target.popover = color(colors.raised);
        target.popover_foreground = color(colors.foreground);
        target.accent = color(colors.selection);
        target.accent_foreground = color(colors.selection_foreground);
        target.list = color(colors.background);
        target.list_hover = color(colors.hover);
        target.list_active = color(colors.selection);
        target.list_active_border = color(colors.selection);
        target.secondary = color(colors.raised);
        target.secondary_hover = color(colors.hover);
        target.secondary_foreground = color(colors.foreground);
        target.primary = color(colors.primary);
        target.title_bar = color(colors.chrome);
        target.title_bar_border = color(colors.hairline);
        target.status_bar = color(colors.chrome);
        target.status_bar_border = color(colors.hairline);
        target.sidebar = color(colors.chrome);
        target.sidebar_border = color(colors.hairline);
        target.tab_bar = color(colors.background);
        target.tab = color(colors.background);
        target.tab_active = color(colors.background);
        target.tab_foreground = color(colors.muted_foreground);
        target.tab_active_foreground = color(colors.foreground);
        target.danger = color(colors.destructive);
        target.warning = color(colors.warning);
        target.info = color(colors.info);
        target.scrollbar_thumb = color(colors.border);
        target.window_border = color(colors.hairline);
        theme.font_size = px(file.typography.body);
        theme.mono_font_size = px(file.typography.mono);
        theme.radius = px(file.radius.md);
        theme.radius_lg = px(file.radius.lg);
        theme.shadow = false;
    });
    cx.set_global(Palette {
        hairline: color(colors.hairline),
        tertiary: color(colors.tertiary_foreground),
        focus: color(colors.focus),
        selection: color(colors.selection),
        selection_foreground: color(colors.selection_foreground),
        hover: color(colors.hover),
        chrome: color(colors.chrome),
        raised: color(colors.raised),
        warning: color(colors.warning),
        destructive: color(colors.destructive),
        row_height: px(file.density.result_row_compact),
    });
}
