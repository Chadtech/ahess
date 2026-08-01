#![allow(dead_code)]

use gpui::{prelude::*, Div, Pixels, Rgba};

use crate::palette;

pub const FONT: &str = "Fira Code";

pub const TEXT_SIZE: Pixels = gpui::px(14.0);
pub const TEXT_LINE_HEIGHT: Pixels = gpui::px(20.0);

pub const S0: Pixels = gpui::px(0.0);
pub const S1: Pixels = gpui::px(1.0);
pub const S2: Pixels = gpui::px(2.0);
pub const S3: Pixels = gpui::px(4.0);
pub const S4: Pixels = gpui::px(8.0);
pub const S5: Pixels = gpui::px(16.0);
pub const S6: Pixels = gpui::px(32.0);
pub const S7: Pixels = gpui::px(64.0);
pub const S8: Pixels = gpui::px(128.0);
pub const S9: Pixels = gpui::px(256.0);
pub const S10: Pixels = gpui::px(512.0);
pub const S11: Pixels = gpui::px(1024.0);

pub const GREEN1: Rgba = rgba(palette::GREEN1);
pub const GREEN2: Rgba = rgba(palette::GREEN2);
pub const GREEN3: Rgba = rgba(palette::GREEN3);
pub const GREEN4: Rgba = rgba(palette::GREEN4);
pub const GREEN5: Rgba = rgba(palette::GREEN5);
pub const GREEN6: Rgba = rgba(palette::GREEN6);
pub const GREEN7: Rgba = rgba(palette::GREEN7);

pub const GRAY1: Rgba = rgba(palette::GRAY1);
pub const GRAY2: Rgba = rgba(palette::GRAY2);
pub const GRAY3: Rgba = rgba(palette::GRAY3);
pub const GRAY4: Rgba = rgba(palette::GRAY4);
pub const GRAY5: Rgba = rgba(palette::GRAY5);
pub const GRAY6: Rgba = rgba(palette::GRAY6);

pub const YELLOW1: Rgba = rgba(palette::YELLOW1);
pub const YELLOW2: Rgba = rgba(palette::YELLOW2);
pub const YELLOW3: Rgba = rgba(palette::YELLOW3);
pub const YELLOW4: Rgba = rgba(palette::YELLOW4);
pub const YELLOW5: Rgba = rgba(palette::YELLOW5);
pub const YELLOW6: Rgba = rgba(palette::YELLOW6);

pub const BLUE1: Rgba = rgba(palette::BLUE1);
pub const BLUE2: Rgba = rgba(palette::BLUE2);

pub const RED1: Rgba = rgba(palette::RED1);
pub const RED2: Rgba = rgba(palette::RED2);

pub const WHITE: Rgba = rgba(palette::WHITE);

// Semantic text colors. Most text should use TEXT_DEFAULT. Brighter text is
// reserved for interaction feedback, while labels and titles are darker.
pub const TEXT_DEFAULT: Rgba = GRAY5;
pub const TEXT_HEADER: Rgba = GRAY4;
pub const TEXT_HOVERED: Rgba = WHITE;
pub const FIELD_LABEL_TEXT: Rgba = GRAY5;
pub const BUTTON_TEXT: Rgba = GRAY6;
pub const DIALOG_TITLE_TEXT: Rgba = GRAY2;
pub const PLAYBACK_ROW_BORDER: Rgba = GRAY5;

pub const CONTENT_PADDING: Pixels = S5;
pub const MODAL_BACKDROP: Rgba = Rgba {
    r: GREEN1.r,
    g: GREEN1.g,
    b: GREEN1.b,
    a: 0.82,
};

pub fn raised(child: impl IntoElement) -> Div {
    raised_with_border(child, GRAY3, GRAY1)
}

pub fn raised_with_border(child: impl IntoElement, light_border: Rgba, dark_border: Rgba) -> Div {
    gpui::div()
        .relative()
        .child(child)
        .child(bevel_top(light_border))
        .child(bevel_left(light_border))
        .child(bevel_bottom(dark_border))
        .child(bevel_right(dark_border))
}

pub fn sunken(child: impl IntoElement) -> Div {
    sunken_with_border(child, GRAY3, GRAY1)
}

pub fn sunken_with_border(child: impl IntoElement, light_border: Rgba, dark_border: Rgba) -> Div {
    gpui::div()
        .relative()
        .child(child)
        .child(bevel_top(dark_border))
        .child(bevel_left(dark_border))
        .child(bevel_bottom(light_border))
        .child(bevel_right(light_border))
}

const fn rgba(color: palette::Color) -> Rgba {
    let hex = color.rgb();
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;

    Rgba { r, g, b, a: 1.0 }
}

fn bevel_top(color: Rgba) -> impl IntoElement {
    gpui::div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(S2)
        .bg(color)
}

fn bevel_left(color: Rgba) -> impl IntoElement {
    gpui::div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .w(S2)
        .bg(color)
}

fn bevel_bottom(color: Rgba) -> impl IntoElement {
    gpui::div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(S2)
        .bg(color)
}

fn bevel_right(color: Rgba) -> impl IntoElement {
    gpui::div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(S2)
        .bg(color)
}
